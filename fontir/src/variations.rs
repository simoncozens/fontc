//! Helps manipulate variation data.
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
    ops::{Mul, Sub},
};

use log::{log_enabled, trace};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    coords::{NormalizedCoord, NormalizedLocation},
    error::VariationModelError,
};

const ZERO: OrderedFloat<f32> = OrderedFloat(0.0);
const ONE: OrderedFloat<f32> = OrderedFloat(1.0);

/// A model of how variation space is subdivided into regions to create deltas.
///
/// Given a set of master locations, figures out a set of regions and the weights each
/// region assigns to each master. This enables us to compute deltas for variation stores.
///
/// See `class VariationModel` in <https://github.com/fonttools/fonttools/blob/main/Lib/fontTools/varLib/models.py>
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VariationModel {
    default: NormalizedLocation,

    // TODO: why isn't this a Map<Loc, Region>
    // All Vec's have same length and items at the same index refer to the same master
    // Which sounds a *lot* like we should have a Vec or Map of Some Struct.
    locations: Vec<NormalizedLocation>,
    influence: Vec<VariationRegion>,

    // [n] gives a vec of (master index, scale for deltas from that master)
    delta_weights: Vec<Vec<(usize, OrderedFloat<f32>)>>,
}

impl VariationModel {
    /// Create a model of variation space subdivision suitable for delta construction.
    ///
    /// Locations should be points in variation space where we wish to define something, such as a
    /// glyph instance.
    ///
    /// Axis order should reflect the importance of the axis.
    pub fn new(
        locations: HashSet<NormalizedLocation>,
        axis_order: Vec<String>,
    ) -> Result<Self, VariationModelError> {
        let axes = axis_order.iter().collect::<HashSet<&String>>();
        let default = axes
            .iter()
            .map(|axis_name| (axis_name.to_string(), NormalizedCoord::new(ZERO)))
            .collect();

        let mut expanded_locations = HashSet::new();
        for mut location in locations.into_iter() {
            // Only axes that have an assigned order are valid
            let bad_axes = location
                .axis_names()
                .filter(|n| !axes.contains(n))
                .cloned()
                .collect::<Vec<_>>();
            if !bad_axes.is_empty() {
                return Err(VariationModelError::AxesWithoutAssignedOrder {
                    axis_names: bad_axes,
                    location: location.clone(),
                });
            }

            // Assign the normalized location 0 for every axis where no other value was assigned
            for axis_name in axes.iter() {
                if !location.has(axis_name) {
                    let axis_name = *axis_name;
                    location.set_pos(axis_name.clone(), NormalizedCoord::new(0.0));
                }
            }

            expanded_locations.insert(location);
        }

        // sort locations for reasons as yet unclear
        let mut locations: Vec<_> = expanded_locations.into_iter().collect();
        let sorting_hat = LocationSortingHat::new(&locations, axis_order);
        locations.sort_by_cached_key(|loc| sorting_hat.key_for(loc));

        let regions = regions_for(&locations);
        let influence = master_influence(regions);
        let delta_weights = delta_weights(&locations, &influence);

        Ok(VariationModel {
            default,
            locations,
            influence,
            delta_weights,
        })
    }

    pub fn empty() -> Self {
        VariationModel {
            default: NormalizedLocation::new(),
            locations: Vec::new(),
            influence: Vec::new(),
            delta_weights: Vec::new(),
        }
    }

    pub fn locations(&self) -> impl Iterator<Item = &NormalizedLocation> {
        self.locations.iter()
    }

    pub fn default_location(&self) -> &NormalizedLocation {
        &self.default
    }

    /// Convert absolute positions at master locations to offsets.
    ///
    /// <ul>
    ///     <li>All keys in points must be known to the variation model.</li>
    ///     <li>All point vectors must have the same length.</li>
    ///     <li>A point vector for the default location is required</li>
    /// </ul>
    ///
    /// Note that it is NOT required to provide a point sequence for every location known
    /// to the variation model.
    ///
    /// P is the point type, meant to be absolute position in 1 or 2 dimensional space.
    /// V is the vector type.
    ///
    /// For 2d [kurbo::Point] and [kurbo::Vec2] would be typical choices. For 1d a floating
    /// point primitive should suffice.
    ///
    /// Returns a delta, as the vector type, for every input point. Intended use is to support
    /// construction of a variation store.
    ///
    /// Rust version of <https://github.com/fonttools/fonttools/blob/3b9a73ff8379ab49d3ce35aaaaf04b3a7d9d1655/Lib/fontTools/varLib/models.py#L449-L461>
    pub fn deltas<P, V>(
        &self,
        point_seqs: &HashMap<NormalizedLocation, Vec<P>>,
    ) -> Result<HashMap<NormalizedLocation, Vec<V>>, DeltaError>
    where
        P: Copy + Default + Sub<P, Output = V>,
        V: Copy + Mul<f64, Output = V> + Sub<V, Output = V>,
    {
        let Some(defaults) = point_seqs.get(&self.default) else {
            return Err(DeltaError::DefaultUndefined);
        };
        if point_seqs.values().any(|pts| pts.len() != defaults.len()) {
            return Err(DeltaError::InconsistentNumbersOfPoints);
        }

        let mut result: HashMap<NormalizedLocation, Vec<V>> = HashMap::new();
        // self.locations is sorted such that[i] is only influenced by[i+1..N]
        // so we know subsequent spins won't invalidate our delta if we go in the same order
        for (loc, points, master_influences) in
            self.locations
                .iter()
                .enumerate()
                .filter_map(|(loc_idx, loc)| {
                    point_seqs
                        .get(loc)
                        .map(|points| (loc, points, &self.delta_weights[loc_idx]))
                })
        {
            let mut deltas = Vec::new();
            for (idx, point) in points.iter().enumerate() {
                let initial_vector: V = *point - Default::default();
                deltas.push(
                    // Find other masters that are active (have influence)
                    // Any master with influence on us was processed already so we can get that masters
                    // deltas from the results so far. If we subtract away all such influences what's
                    // left is the delta to take us to point.
                    master_influences
                        .iter()
                        .map(|(_, master_weight)| master_weight)
                        .zip(&self.locations)
                        .filter_map(|(master_weight, master_loc)| {
                            let Some(master_deltas) = result.get(master_loc) else {
                                return None;
                            };
                            let Some(delta) = master_deltas.get(idx) else {
                                return None;
                            };
                            Some((delta, master_weight.into_inner()))
                        })
                        .fold(initial_vector, |acc, (other, other_weight)| {
                            acc - if other_weight != 1.0 {
                                *other * other_weight.into()
                            } else {
                                *other
                            }
                        }),
                );
            }
            result.insert(loc.clone(), deltas);
        }

        Ok(result)
    }
}

#[derive(Error, Debug)]
pub enum DeltaError {
    #[error("The default must have a point sequence")]
    DefaultUndefined,
    #[error("Every point sequence must have the same length")]
    InconsistentNumbersOfPoints,
    #[error("{0:?} is not present in the variation model")]
    UnknownLocation(NormalizedLocation),
}

/// Gryffindor!
///
/// Sorts locations, and thus the resulting regions, based on the intuition that from most
/// to least influential are the default master, then corner masters, and finally
/// any other masters (sometimes referred to as knockout or fixup masters). This is computed
/// based on the master location being offset from default on fewer axes rather than more
///
/// * This is [LocationSortKey.rank] and [LocationSortKey.on_axis_points]
/// * The default master will thus come first, followed by any masters directly on axes
/// * For example, if default is weight 400, width 100 then the master at weight 700, width 100
///   will sort before the master at weight 700, weight 75
///
/// Additional sorting is applied after this to ensure a deteriministic order in case of ties.
///
/// This algorithm started in MutatorMath and was subsequently modified in FontTools.
///
/// This is a Rust version of FontTools getMasterLocationsSortKeyFunc.
/// <https://github.com/fonttools/fonttools/blob/2f1f5e5e7be331d960a0e30d537c2b4c70d89285/Lib/fontTools/varLib/models.py#L295>
struct LocationSortingHat {
    axis_order: Vec<String>,
    on_axis_points: HashMap<String, HashSet<NormalizedCoord>>,
}

impl LocationSortingHat {
    fn new(locations: &[NormalizedLocation], axis_order: Vec<String>) -> LocationSortingHat {
        // Location is on-axis if it has exactly 1 non-zero coordinate
        let mut on_axis_points: HashMap<String, HashSet<NormalizedCoord>> = HashMap::new();
        'location: for location in locations {
            let mut on_axis: Option<(&String, NormalizedCoord)> = None;
            for (axis_name, pos) in location.iter() {
                if pos.into_inner() == ZERO {
                    continue;
                }
                if on_axis.is_some() {
                    continue 'location; // multiple non-zero coords, bail out
                }
                on_axis = Some((axis_name, *pos));
            }
            if let Some((axis_name, pos)) = on_axis {
                on_axis_points
                    .entry(axis_name.clone())
                    .or_default()
                    .insert(pos);
            }
        }
        LocationSortingHat {
            axis_order,
            on_axis_points,
        }
    }

    fn key_for(&self, location: &NormalizedLocation) -> LocationSortKey {
        let mut rank = 0;
        let mut on_axis_points: i16 = 0;
        let mut ordered_axes = Vec::new();
        let mut non_zero_axes = Vec::new();

        // Score the index of the axis for any axis with non-zero value in the user specified axis order
        let mut known_axes = Vec::new();
        for (idx, axis_name) in self.axis_order.iter().enumerate() {
            if location.has_non_zero(axis_name) {
                known_axes.push(idx);
                ordered_axes.push(axis_name.clone());
            }
        }

        for (axis_name, pos) in location.iter() {
            if pos.into_inner() != ZERO {
                // Rank is the # of axes with non-zero values
                rank += 1;

                // Score more than the max allowable axes for any axis with non-zero value that has no user-specified order
                // This shouldn't hapen because it's an AxesWithoutAssignedOrder error in ctor, retained
                // because I want key_for to be infallible.
                if !self.axis_order.contains(axis_name) {
                    known_axes.push(0x10000);
                }

                non_zero_axes.push(axis_name);
            }
            // score -1 for every axis position that matches a location that was directly on an axis
            on_axis_points += self
                .on_axis_points
                .get(axis_name)
                .map(|on_axis| on_axis.get(pos).map_or(0, |_| -1))
                .unwrap_or_default();
        }

        let mut unordered_axes: Vec<String> = non_zero_axes
            .into_iter()
            .filter(|axis_name| !ordered_axes.contains(axis_name))
            .cloned()
            .collect();
        unordered_axes.sort();
        ordered_axes.extend(unordered_axes);

        let axis_value_signs = ordered_axes
            .iter()
            .map(|axis_name| {
                location
                    .get(axis_name)
                    .map(|coord| match coord.into_inner().cmp(&ZERO) {
                        Ordering::Greater => 1_i8,
                        Ordering::Less => -1_i8,
                        Ordering::Equal => 0_i8,
                    })
                    .unwrap_or_default()
            })
            .collect();

        let axis_value_abs = ordered_axes
            .iter()
            .map(|axis_name| {
                location
                    .get(axis_name)
                    .map(|coord| OrderedFloat(coord.into_inner().abs()))
                    .unwrap_or_default()
            })
            .collect();

        let result = LocationSortKey {
            rank,
            on_axis_points,
            known_axes,
            ordered_axes,
            axis_value_signs,
            axis_value_abs,
        };
        trace!("key for {:?} is {:?}", location, result);

        result
    }
}

/// Sort key for a location.
///
/// Only axes with a non-zero normalized value are of interest.
///
/// <https://github.com/fonttools/fonttools/blob/2f1f5e5e7be331d960a0e30d537c2b4c70d89285/Lib/fontTools/varLib/models.py#L326>
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LocationSortKey {
    // 1 for every non-zero entry in the map
    rank: usize,
    // -1 for every axis:value entry in location where a location exists whose only non-zero
    // position is on axis at value. That is, a location exists that is directly on the axis there.
    on_axis_points: i16,
    // index in user-specified axis order, otherwise score 0x10000 (65536)
    known_axes: Vec<usize>,
    // ordered list of axis in location
    ordered_axes: Vec<String>,
    // signs of positions on axes in ordered_axes order
    axis_value_signs: Vec<i8>,
    // abs values of positions on axes in ordered_axes order
    axis_value_abs: Vec<OrderedFloat<f32>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct VariationRegion(HashMap<String, Tent>);

impl VariationRegion {
    fn new() -> Self {
        VariationRegion(HashMap::new())
    }
    /// The scalar multiplier for the provided location for this region
    ///
    /// Returns None for no influence, Some(non-zero value) for influence.
    ///
    /// In Python, supportScalar. We only implement the ot=True, extrapolate=False paths.
    /// <https://github.com/fonttools/fonttools/blob/2f1f5e5e7be331d960a0e30d537c2b4c70d89285/Lib/fontTools/varLib/models.py#L123>.
    fn scalar_at(&self, location: &NormalizedLocation) -> OrderedFloat<f32> {
        let scalar =
            self.0
                .iter()
                .filter(|(_, ar)| ar.validate())
                .fold(ONE, |scalar, (axis_name, tent)| {
                    let v = location
                        .get(axis_name)
                        .map(|v| v.into_inner())
                        .unwrap_or_default();
                    let lower = tent.lower.into_inner();
                    let peak = tent.peak.into_inner();
                    let upper = tent.upper.into_inner();

                    // If we're at the peak by definition we have full influence
                    if v == peak {
                        return scalar; // *= 1
                    }

                    // If the Tent is 0,0,0 it's always in full effect
                    if (lower, peak, upper) == (ZERO, ZERO, ZERO) {
                        return scalar; // *= 1
                    }

                    if v <= lower || upper <= v {
                        trace!(
                            "  {:?} => 0 due to {} {:?} at {:?}",
                            self,
                            axis_name,
                            tent,
                            location
                        );
                        return ZERO;
                    }

                    let subtract_me = if v < peak {
                        tent.lower.into_inner()
                    } else {
                        tent.upper.into_inner()
                    };
                    scalar * (v - subtract_me) / (peak - subtract_me)
                });
        scalar
    }
}

/// The min/peak/max of a masters influence.
///
/// Visualize as a tent of influence, starting at lower, peaking at peak,
/// and dropping off to zero at upper.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Tent {
    lower: NormalizedCoord,
    peak: NormalizedCoord,
    upper: NormalizedCoord,
}

impl Tent {
    fn new(mut lower: NormalizedCoord, peak: NormalizedCoord, mut upper: NormalizedCoord) -> Self {
        let zero = NormalizedCoord::new(0.0);
        if peak > zero {
            lower = zero;
        } else {
            upper = zero;
        }
        Tent { lower, peak, upper }
    }

    /// OT-specific validation of whether we could have any influence
    ///
    /// (0,0,0) IS valid, meaning apply my deltas at full scale always
    ///
    /// <https://github.com/fonttools/fonttools/blob/2f1f5e5e7be331d960a0e30d537c2b4c70d89285/Lib/fontTools/varLib/models.py#L162>
    fn validate(&self) -> bool {
        let lower = self.lower.into_inner();
        let peak = self.peak.into_inner();
        let upper = self.upper.into_inner();

        if lower > peak || peak > upper {
            return false;
        }
        // In fonts the influence at zero must be zero so we cannot span zero
        if lower < ZERO && upper > ZERO {
            return false;
        }
        true
    }
}

impl Debug for Tent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{self}"))
    }
}

impl Display for Tent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let comment = if self.validate() { "" } else { " (invalid)" };
        f.write_fmt(format_args!(
            "Tent {{{}, {}, {}{}}}",
            self.lower.into_inner(),
            self.peak.into_inner(),
            self.upper.into_inner(),
            comment
        ))?;
        Ok(())
    }
}

impl From<(f32, f32, f32)> for Tent {
    fn from(value: (f32, f32, f32)) -> Self {
        Tent::new(
            NormalizedCoord::new(value.0),
            NormalizedCoord::new(value.1),
            NormalizedCoord::new(value.2),
        )
    }
}

/// Split space into regions.
///
/// VariationModel::_locationsToRegions in Python.
/// <https://github.com/fonttools/fonttools/blob/2f1f5e5e7be331d960a0e30d537c2b4c70d89285/Lib/fontTools/varLib/models.py#L416>
fn regions_for(locations: &[NormalizedLocation]) -> Vec<VariationRegion> {
    let mut minmax = HashMap::<&String, (NormalizedCoord, NormalizedCoord)>::new();
    for location in locations.iter() {
        for (axis, value) in location.iter() {
            let (min, max) = minmax
                .entry(axis)
                .or_insert_with(|| (NormalizedCoord::new(0.0), NormalizedCoord::new(0.0)));
            if value < min {
                *min = *value;
            }
            if value > max {
                *max = *value;
            }
        }
    }

    locations
        .iter()
        .map(|location| {
            let mut region = VariationRegion::new();
            for (axis, value) in location.iter() {
                // Python just scrubs 0's out of the location's. We elect to store representative tents.
                let (min, max) = if value.into_inner() == ZERO {
                    (NormalizedCoord::new(ZERO), NormalizedCoord::new(ZERO))
                } else {
                    *minmax.get(axis).unwrap()
                };
                region.0.insert(axis.clone(), Tent::new(min, *value, max));
            }
            region
        })
        .collect()
}

/// Compute the influence of each master, if any, on each region.
///
/// The regions must have been sorted by the [LocationSortingHat] as this ensures
/// that for each region we only need to look at preceeding regions for overlaps.
///
/// VariationModel::_computeMasterSupports in Python.
/// <https://github.com/fonttools/fonttools/blob/2f1f5e5e7be331d960a0e30d537c2b4c70d89285/Lib/fontTools/varLib/models.py#L360>
fn master_influence(regions: Vec<VariationRegion>) -> Vec<VariationRegion> {
    let mut influence = Vec::new();
    for (i, region) in regions.iter().enumerate() {
        let axes: HashSet<&String> = region.0.keys().collect();
        let mut region = region.clone();
        for prev_region in regions[..i].iter() {
            if axes != prev_region.0.keys().collect() {
                continue;
            }
            // If prev doesn't overlap current we aren't interested
            let overlap = region.0.iter().all(|(axis_name, tent)| {
                let prev_peak = prev_region.0[axis_name].peak;
                prev_peak == tent.peak || (tent.lower < prev_peak && prev_peak < tent.upper)
            });
            if !overlap {
                continue;
            }

            // As in Python version, split the box for the new master in the direction with the
            // largest range ratio. Cut across multiple axes if they have the largest equal ratio.
            // https://github.com/fonttools/fonttools/commit/7ee81c8821671157968b097f3e55309a1faa511e#commitcomment-31054804
            let mut axis_regions: HashMap<&String, Tent> = HashMap::new();
            let mut best_ratio = OrderedFloat(-1.0);
            for axis in axes.iter() {
                let prev_peak = prev_region.0[*axis].peak;
                let mut axis_region = region.0[*axis].clone();
                let ratio;
                match prev_peak.cmp(&axis_region.peak) {
                    Ordering::Less => {
                        ratio = (prev_peak - axis_region.peak).into_inner()
                            / (axis_region.lower - axis_region.peak).into_inner();
                        axis_region.lower = prev_peak;
                    }
                    Ordering::Greater => {
                        ratio = (prev_peak - axis_region.peak).into_inner()
                            / (axis_region.upper - axis_region.peak).into_inner();
                        axis_region.upper = prev_peak;
                    }
                    Ordering::Equal => continue, // can't split in this direction
                }
                if ratio > best_ratio {
                    axis_regions.clear();
                    best_ratio = ratio;
                }
                if ratio == best_ratio {
                    axis_regions.insert(axis, axis_region);
                }
            }
            for (axis, axis_region) in axis_regions {
                region.0.insert(axis.to_owned(), axis_region);
            }
        }
        influence.push(region);
    }
    influence
}

/// Figure out the multipliers to use when applying for deltas from masters.
///
/// VariationModel::_computeDeltaWeights in Python.
/// <https://github.com/fonttools/fonttools/blob/2f1f5e5e7be331d960a0e30d537c2b4c70d89285/Lib/fontTools/varLib/models.py#L438>
fn delta_weights(
    locations: &[NormalizedLocation],
    influencers: &[VariationRegion],
) -> Vec<Vec<(usize, OrderedFloat<f32>)>> {
    if log_enabled!(log::Level::Trace) {
        for (l, i) in locations.iter().zip(influencers) {
            trace!("{:?}", l);
            for (axis_name, tent) in i.0.iter() {
                trace!("  {} {}", axis_name, tent);
            }
        }
    }
    trace!("Delta Weights");
    let mut weights = Vec::new();
    for (loc_idx, location) in locations.iter().enumerate() {
        weights.push(
            influencers[..loc_idx]
                .iter()
                .enumerate()
                .filter_map(|(inf_idx, influence)| {
                    let scalar = influence.scalar_at(location);
                    if scalar == ZERO {
                        trace!(
                            "  no influence: {} {:?} at {:?}",
                            inf_idx,
                            influence,
                            location
                        );
                        return None;
                    }
                    Some((inf_idx, scalar))
                })
                .collect(),
        );
        trace!("  {} {:?}", loc_idx, weights.last().unwrap());
    }
    weights
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use kurbo::{Point, Vec2};
    use ordered_float::OrderedFloat;

    use pretty_assertions::assert_eq;

    use crate::{
        coords::{NormalizedCoord, NormalizedLocation},
        variations::ONE,
    };

    use super::{VariationModel, VariationRegion};

    fn norm_loc(positions: &[(&str, f32)]) -> NormalizedLocation {
        positions
            .iter()
            .map(|(axis_name, value)| (axis_name.to_string(), NormalizedCoord::new(*value)))
            .collect()
    }

    fn default_master_weight() -> Vec<(usize, OrderedFloat<f32>)> {
        // no locations are contributing deltas
        Vec::new()
    }

    /// Python
    /// >>> supportScalar({}, {})
    /// 1.0
    #[test]
    fn scalar_at_default_for_default() {
        let loc = NormalizedLocation::new();
        assert_eq!(ONE, VariationRegion::new().scalar_at(&loc));
    }

    /// Python
    /// >>> supportScalar({'wght':.2}, {})
    /// 1.0
    #[test]
    fn scalar_at_off_default_for_default() {
        let loc = norm_loc(&[("Weight", 0.2)]);
        assert_eq!(ONE, VariationRegion::new().scalar_at(&loc));
    }

    /// Python
    /// >>> supportScalar({'wght':.2}, {'wght':(0,2,3)})
    /// 0.1
    #[test]
    fn scalar_at_off_default_for_simple_weight() {
        let loc = norm_loc(&[("Weight", 0.2)]);
        let mut region = VariationRegion::new();
        region
            .0
            .insert("Weight".to_string(), (0.0, 2.0, 3.0).into());
        assert_eq!(OrderedFloat(0.1), region.scalar_at(&loc));
    }

    /// Python
    /// >>> supportScalar({'wght':2.5}, {'wght':(0,2,4)})
    /// 0.75
    #[test]
    fn scalar_at_for_weight() {
        let loc = norm_loc(&[("Weight", 2.5)]);
        let mut region = VariationRegion::new();
        region
            .0
            .insert("Weight".to_string(), (0.0, 2.0, 4.0).into());
        assert_eq!(OrderedFloat(0.75), region.scalar_at(&loc));
    }

    /// Python
    /// >>> supportScalar({'wght':2.5, 'wdth':0}, {'wght':(0,2,4), 'wdth':(-1,0,+1)})
    /// 0.75
    /// Note that under font rules a peak of 0 means no influence
    #[test]
    fn scalar_at_for_weight_width_fixup() {
        let loc = norm_loc(&[("Weight", 2.5), ("Width", 0.0)]);
        let mut region = VariationRegion::new();
        region
            .0
            .insert("Weight".to_string(), (0.0, 2.0, 4.0).into());
        region
            .0
            .insert("Width".to_string(), (-1.0, 0.0, 1.0).into());
        assert_eq!(OrderedFloat(0.75), region.scalar_at(&loc));
    }

    /// Python
    /// >>> supportScalar({'wght':1, 'wdth':1}, {'wght':(0, 1, 1)})
    /// 1.0
    #[test]
    fn scalar_at_for_weight_width_corner() {
        let loc = norm_loc(&[("Weight", 1.0), ("Width", 1.0)]);
        let mut region = VariationRegion::new();
        region
            .0
            .insert("Weight".to_string(), (0.0, 1.0, 1.0).into());
        assert_eq!(OrderedFloat(1.0), region.scalar_at(&loc));
    }

    /// >>> models.VariationModel([{'wght':0}]).locations
    /// [{}]
    /// >>> pprint(models.VariationModel([{'wght':0}]).deltaWeights)
    /// [{}]    
    #[test]
    fn delta_weights_for_static_family_one_axis() {
        let loc = NormalizedLocation::new();
        let locations = HashSet::from([loc]);
        let axis_order = vec!["Weight".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(vec![norm_loc(&[("Weight", 0.0)])], model.locations);
        assert_eq!(vec![default_master_weight()], model.delta_weights);
    }

    /// >>> models.VariationModel([{'wght':0, 'ital': 0, 'wdth': 0}]).locations
    /// [{}]
    /// >>> pprint(models.VariationModel([{'wght':0, 'ital': 0, 'wdth': 0}]).deltaWeights)
    /// [{}]
    #[test]
    fn delta_weights_for_static_family_many_axes() {
        let loc = norm_loc(&[("Weight", 0.0), ("Italic", 0.0), ("Width", 0.0)]);
        let locations = HashSet::from([loc.clone()]);
        let axis_order = vec![
            "Width".to_string(),
            "Weight".to_string(),
            "Italic".to_string(),
        ];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(vec![loc], model.locations);
        assert_eq!(vec![default_master_weight()], model.delta_weights);
    }

    /// # two-master weight family
    /// >>> models.VariationModel([{'wght':0}, {'wght': 1}]).locations
    /// [{}, {'wght': 1}]
    /// >>> pprint(models.VariationModel([{'wght':0}, {'wght': 1}]).deltaWeights)
    /// [{}, {0: 1.0}]
    #[test]
    fn delta_weights_for_2_master_weight_variable_family() {
        let weight_0 = norm_loc(&[("Weight", 0.0)]);
        let weight_1 = norm_loc(&[("Weight", 1.0)]);
        let locations = HashSet::from([weight_1.clone(), weight_0.clone()]);
        let axis_order = vec!["Weight".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(vec![weight_0, weight_1], model.locations);
        assert_eq!(
            vec![default_master_weight(), vec![(0_usize, OrderedFloat(1.0))]],
            model.delta_weights
        );
    }

    /// # three-master weight family
    /// >>> models.VariationModel([{'wght':-1}, {'wght': 0}, {'wght': 1}]).locations
    /// [{}, {'wght': -1}, {'wght': 1}]
    /// >>> models.VariationModel([{'wght':-1}, {'wght': 0}, {'wght': 1}]).deltaWeights
    /// [{}, {0: 1.0}, {0: 1.0}]    
    #[test]
    fn delta_weights_for_3_master_weight_variable_family() {
        let weight_minus_1 = norm_loc(&[("Weight", -1.0)]);
        let weight_0 = norm_loc(&[("Weight", 0.0)]);
        let weight_1 = norm_loc(&[("Weight", 1.0)]);
        let locations = HashSet::from([weight_1.clone(), weight_0.clone(), weight_minus_1.clone()]);
        let axis_order = vec!["Weight".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(vec![weight_0, weight_minus_1, weight_1], model.locations);
        assert_eq!(
            vec![
                default_master_weight(),
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))]
            ],
            model.delta_weights
        );
    }

    /// # corner-master weight + width
    /// >>> models.VariationModel([{'wght':0, 'wdth': 0}, {'wght':1, 'wdth': 0}, {'wght':0, 'wdth': 1}, {'wght':1, 'wdth': 1}]).locations
    /// [{}, {'wdth': 1}, {'wght': 1}, {'wght': 1, 'wdth': 1}]
    /// >>> models.VariationModel([{'wght':0, 'wdth': 0}, {'wght':1, 'wdth': 0}, {'wght':0, 'wdth': 1}, {'wght':1, 'wdth': 1}]).deltaWeights
    /// [{}, {0: 1.0}, {0: 1.0}, {0: 1.0, 1: 1.0, 2: 1.0}]
    #[test]
    fn delta_weights_for_corner_master_weight_width_family() {
        let wght0_wdth0 = norm_loc(&[("Weight", 0.0), ("Width", 0.0)]);
        let wght0_wdth1 = norm_loc(&[("Weight", 0.0), ("Width", 1.0)]);
        let wght1_wdth0 = norm_loc(&[("Weight", 1.0), ("Width", 0.0)]);
        let wght1_wdth1 = norm_loc(&[("Weight", 1.0), ("Width", 1.0)]);
        let locations = HashSet::from([
            wght0_wdth0.clone(),
            wght0_wdth1.clone(),
            wght1_wdth0.clone(),
            wght1_wdth1.clone(),
        ]);
        let axis_order = vec!["Weight".to_string(), "Width".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(
            vec![wght0_wdth0, wght1_wdth0, wght0_wdth1, wght1_wdth1],
            model.locations
        );
        assert_eq!(
            vec![
                default_master_weight(),
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (1_usize, OrderedFloat(1.0)),
                    (2_usize, OrderedFloat(1.0))
                ],
            ],
            model.delta_weights
        );
    }

    /// # corner-master weight + width rig with a default master and a fixup
    /// >>> models.VariationModel([{'wght':0, 'wdth': 0}, {'wght':-1, 'wdth': -1}, {'wght':-1, 'wdth': 1}, {'wght':1, 'wdth': -1}, {'wght':1, 'wdth': 1}, {'wght':0.5, 'wdth': 0.5}], axisOrder=['wght', 'wdth']).locations
    /// [{}, {'wght': -1, 'wdth': -1}, {'wght': -1, 'wdth': 1}, {'wght': 1, 'wdth': -1}, {'wght': 0.5, 'wdth': 0.5}, {'wght': 1, 'wdth': 1}]
    /// >>> models.VariationModel([{'wght':0, 'wdth': 0}, {'wght':-1, 'wdth': -1}, {'wght':-1, 'wdth': 1}, {'wght':1, 'wdth': -1}, {'wght':1, 'wdth': 1}, {'wght':0.5, 'wdth': 0.5}], axisOrder=['wght', 'wdth']).deltaWeights
    /// [{}, {0: 1.0}, {0: 1.0}, {0: 1.0}, {0: 1.0}, {0: 1.0}]
    #[test]
    fn delta_weights_for_corner_default_and_fixup_master_weight_width_family() {
        let default_master = norm_loc(&[("Weight", 0.0), ("Width", 0.0)]);
        let min_wght_min_wdth = norm_loc(&[("Weight", -1.0), ("Width", -1.0)]);
        let min_wght_max_wdth = norm_loc(&[("Weight", -1.0), ("Width", 1.0)]);
        let max_wght_min_wdth = norm_loc(&[("Weight", 1.0), ("Width", -1.0)]);
        let max_wght_max_wdth = norm_loc(&[("Weight", 1.0), ("Width", 1.0)]);
        let fixup = norm_loc(&[("Weight", 0.5), ("Width", 0.5)]);
        let locations = HashSet::from([
            // Default master
            default_master.clone(),
            // Corners: the far ends of each axis
            max_wght_max_wdth.clone(),
            max_wght_min_wdth.clone(),
            min_wght_max_wdth.clone(),
            min_wght_min_wdth.clone(),
            // A fixup or knockout
            fixup.clone(),
        ]);
        let axis_order = vec!["Weight".to_string(), "Width".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(
            vec![
                default_master,
                min_wght_min_wdth,
                min_wght_max_wdth,
                max_wght_min_wdth,
                fixup,
                max_wght_max_wdth
            ],
            model.locations
        );
        assert_eq!(
            vec![
                default_master_weight(),
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
            ],
            model.delta_weights
        );
    }

    /// A scenario from models_test.py::test_init
    ///
    /// <https://github.com/fonttools/fonttools/blob/bf265ce49e0cae6f032420a4c80c31d8e16285b8/Tests/varLib/models_test.py#L199>
    #[test]
    fn delta_weights_for_many_master_weight_width_family() {
        let locations = HashSet::from([
            norm_loc(&[("Weight", 0.55), ("Width", 0.0)]),
            norm_loc(&[("Weight", -0.55), ("Width", 0.0)]),
            norm_loc(&[("Weight", -1.0), ("Width", 0.0)]),
            norm_loc(&[("Weight", 0.0), ("Width", 1.0)]),
            norm_loc(&[("Weight", 0.66), ("Width", 1.0)]),
            norm_loc(&[("Weight", 0.66), ("Width", 0.66)]),
            norm_loc(&[("Weight", 0.0), ("Width", 0.0)]),
            norm_loc(&[("Weight", 1.0), ("Width", 1.0)]),
            norm_loc(&[("Weight", 1.0), ("Width", 0.0)]),
        ]);
        let axis_order = vec!["Weight".to_string(), "Width".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(
            vec![
                norm_loc(&[("Weight", 0.0), ("Width", 0.0)]),
                norm_loc(&[("Weight", -0.55), ("Width", 0.0)]),
                norm_loc(&[("Weight", -1.0), ("Width", 0.0)]),
                norm_loc(&[("Weight", 0.55), ("Width", 0.0)]),
                norm_loc(&[("Weight", 1.0), ("Width", 0.0)]),
                norm_loc(&[("Weight", 0.0), ("Width", 1.0)]),
                norm_loc(&[("Weight", 1.0), ("Width", 1.0)]),
                norm_loc(&[("Weight", 0.66), ("Width", 1.0)]),
                norm_loc(&[("Weight", 0.66), ("Width", 0.66)]),
            ],
            model.locations
        );

        assert_eq!(
            vec![
                default_master_weight(),
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (4_usize, OrderedFloat(1.0)),
                    (5_usize, OrderedFloat(1.0))
                ],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (3_usize, OrderedFloat(0.7555555)),
                    (4_usize, OrderedFloat(0.24444449)),
                    (5_usize, OrderedFloat(1.0)),
                    (6_usize, OrderedFloat(0.66))
                ],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (3_usize, OrderedFloat(0.7555555)),
                    (4_usize, OrderedFloat(0.24444449)),
                    (5_usize, OrderedFloat(0.66)),
                    (6_usize, OrderedFloat(0.43560004)),
                    (7_usize, OrderedFloat(0.66))
                ],
            ],
            model.delta_weights
        );
    }

    /// A scenario from models_test.py::test_init
    ///
    /// <https://github.com/fonttools/fonttools/blob/bf265ce49e0cae6f032420a4c80c31d8e16285b8/Tests/varLib/models_test.py#L259>
    #[test]
    fn delta_weights_for_foo_bar_family_case_1() {
        let locations = HashSet::from([
            norm_loc(&[("foo", 0.0), ("bar", 0.0)]),
            norm_loc(&[("foo", 0.0), ("bar", 0.5)]),
            norm_loc(&[("foo", 0.0), ("bar", 1.0)]),
            norm_loc(&[("foo", 1.0), ("bar", 0.0)]),
            norm_loc(&[("foo", 1.0), ("bar", 0.5)]),
            norm_loc(&[("foo", 1.0), ("bar", 1.0)]),
        ]);
        let axis_order = vec!["bar".to_string(), "foo".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(
            vec![
                norm_loc(&[("foo", 0.0), ("bar", 0.0)]),
                norm_loc(&[("foo", 0.0), ("bar", 0.5)]),
                norm_loc(&[("foo", 0.0), ("bar", 1.0)]),
                norm_loc(&[("foo", 1.0), ("bar", 0.0)]),
                norm_loc(&[("foo", 1.0), ("bar", 0.5)]),
                norm_loc(&[("foo", 1.0), ("bar", 1.0)]),
            ],
            model.locations
        );
        assert_eq!(
            vec![
                default_master_weight(),
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (1_usize, OrderedFloat(1.0)),
                    (3_usize, OrderedFloat(1.0))
                ],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (2_usize, OrderedFloat(1.0)),
                    (3_usize, OrderedFloat(1.0))
                ],
            ],
            model.delta_weights
        );
    }

    /// A scenario from models_test.py::test_init
    ///
    /// <https://github.com/fonttools/fonttools/blob/bf265ce49e0cae6f032420a4c80c31d8e16285b8/Tests/varLib/models_test.py#L294>
    #[test]
    fn delta_weights_for_foo_bar_family_case_2() {
        let locations = HashSet::from([
            norm_loc(&[("foo", 0.0), ("bar", 0.0)]),
            norm_loc(&[("foo", 0.25), ("bar", 0.0)]),
            norm_loc(&[("foo", 0.5), ("bar", 0.0)]),
            norm_loc(&[("foo", 0.75), ("bar", 0.0)]),
            norm_loc(&[("foo", 1.0), ("bar", 0.0)]),
            norm_loc(&[("foo", 0.0), ("bar", 0.25)]),
            norm_loc(&[("foo", 0.0), ("bar", 0.75)]),
            norm_loc(&[("foo", 0.0), ("bar", 1.0)]),
        ]);
        let axis_order = vec!["bar".to_string(), "foo".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        assert_eq!(
            vec![
                norm_loc(&[("foo", 0.0), ("bar", 0.0)]),
                norm_loc(&[("foo", 0.0), ("bar", 0.25)]),
                norm_loc(&[("foo", 0.0), ("bar", 0.75)]),
                norm_loc(&[("foo", 0.0), ("bar", 1.0)]),
                norm_loc(&[("foo", 0.25), ("bar", 0.0)]),
                norm_loc(&[("foo", 0.5), ("bar", 0.0)]),
                norm_loc(&[("foo", 0.75), ("bar", 0.0)]),
                norm_loc(&[("foo", 1.0), ("bar", 0.0)]),
            ],
            model.locations
        );
        assert_eq!(
            vec![
                default_master_weight(),
                vec![(0_usize, OrderedFloat(1.0))],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (1_usize, OrderedFloat(0.33333334))
                ],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![(0_usize, OrderedFloat(1.0))],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (4_usize, OrderedFloat(0.6666667))
                ],
                vec![
                    (0_usize, OrderedFloat(1.0)),
                    (4_usize, OrderedFloat(0.33333334)),
                    (5_usize, OrderedFloat(0.5))
                ],
                vec![(0_usize, OrderedFloat(1.0))],
            ],
            model.delta_weights
        );
    }

    #[test]
    fn compute_simple_delta_corner_masters() {
        let origin = norm_loc(&[("wght", 0.0), ("wdth", 0.0)]);
        let max_wght = norm_loc(&[("wght", 1.0), ("wdth", 0.0)]);
        let max_wdth = norm_loc(&[("wght", 0.0), ("wdth", 1.0)]);
        let max_wght_wdth = norm_loc(&[("wght", 1.0), ("wdth", 1.0)]);
        let locations = HashSet::from([
            origin.clone(),
            max_wght.clone(),
            max_wdth.clone(),
            max_wght_wdth.clone(),
        ]);
        let axis_order = vec!["wght".to_string(), "wdth".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        let point_seqs = HashMap::from([
            (origin.clone(), vec![Point::new(10.0, 10.0)]),
            (max_wght.clone(), vec![Point::new(12.0, 11.0)]),
            (max_wdth.clone(), vec![Point::new(11.0, 12.0)]),
            (max_wght_wdth.clone(), vec![Point::new(14.0, 11.0)]),
        ]);

        let mut deltas: Vec<_> = model.deltas(&point_seqs).unwrap().into_iter().collect();
        deltas.sort_by_key(|(loc, _)| loc.clone());

        assert_eq!(
            vec![
                // default
                (origin, vec![Vec2::new(10.0, 10.0)]),
                // at max weight/width no other master has influence so it's just delta from default
                (max_wght, vec![Vec2::new(2.0, 1.0)]),
                (max_wdth, vec![Vec2::new(1.0, 2.0)]),
                // at max wdth and wght the deltas for max_wght and max_wdth are in full effect
                // so we get (10+2+1, 10+1+2) "for free" and our delta is from there to (14, 11)
                (max_wght_wdth, vec![Vec2::new(1.0, -2.0)]),
            ],
            deltas
        );
    }

    #[test]
    fn compute_1d_deltas() {
        let origin = norm_loc(&[("wght", 0.0)]);
        let max_wght = norm_loc(&[("wght", 1.0)]);
        let min_wght = norm_loc(&[("wght", -1.0)]);
        let locations = HashSet::from([origin.clone(), max_wght.clone(), min_wght.clone()]);
        let axis_order = vec!["wght".to_string()];
        let model = VariationModel::new(locations, axis_order).unwrap();

        let point_seqs = HashMap::from([
            (origin.clone(), vec![10.0]),
            (max_wght.clone(), vec![12.0]),
            (min_wght.clone(), vec![5.0]),
        ]);

        let mut deltas: Vec<_> = model.deltas(&point_seqs).unwrap().into_iter().collect();
        deltas.sort_by_key(|(loc, _)| loc.clone());
        assert_eq!(
            vec![
                (min_wght, vec![-5.0]),
                (origin, vec![10.0]),
                (max_wght, vec![2.0]),
            ],
            deltas
        );
    }
}
