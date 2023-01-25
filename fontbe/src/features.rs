//! Feature binary compilation.

use std::{
    fmt::Debug,
    fs,
    path::{Path, PathBuf},
};

use fea_rs::{Diagnostic, GlyphMap, GlyphName as FeaRsGlyphName, ParseContext};
use fontir::ir::Features;
use log::{debug, error, trace, warn};
use write_fonts::FontBuilder;

use fontdrasil::orchestration::Work;

use crate::{
    error::Error,
    orchestration::{BeWork, Context},
};

pub struct FeatureWork {
    build_dir: PathBuf,
}

impl FeatureWork {
    pub fn create(build_dir: &Path) -> Box<BeWork> {
        let build_dir = build_dir.to_path_buf();
        Box::new(FeatureWork { build_dir })
    }
}

fn check_diagnostics(
    feature_source: impl Debug,
    op: &str,
    diagnostics: &Vec<Diagnostic>,
    formatter: impl Fn(&Diagnostic) -> String,
) -> Result<(), Error> {
    let mut err = false;
    for diagnostic in diagnostics {
        if diagnostic.is_error() {
            warn!(
                "{:?} {} error {}",
                feature_source,
                op,
                formatter(diagnostic)
            );
            err = true;
        } else {
            debug!("{:?} {} {}", feature_source, op, formatter(diagnostic));
        }
    }
    if err {
        return Err(Error::FeaError(format!(
            "{:?} {} failed",
            feature_source, op
        )));
    }
    Ok(())
}

impl FeatureWork {
    fn compile_parse(
        &self,
        feature_source: &str,
        parse: ParseContext,
        glyph_order: GlyphMap,
    ) -> Result<FontBuilder, Error> {
        let (tree, diagnostics) = parse.generate_parse_tree();
        check_diagnostics(feature_source, "generate parse tree", &diagnostics, |d| {
            format!("{:?}", d)
        })?;

        // Maybe even compile?
        let compilation = match fea_rs::compile::compile(&tree, &glyph_order) {
            Ok(compilation) => {
                check_diagnostics(feature_source, "compile", &compilation.warnings, |d| {
                    tree.format_diagnostic(d)
                })?;
                trace!("Compiled {} successfully", feature_source);
                compilation
            }
            Err(errors) => {
                check_diagnostics(feature_source, "compile", &errors, |d| {
                    tree.format_diagnostic(d)
                })?;
                unreachable!("errors aren't ... errors?!");
            }
        };

        // Capture the binary tables we got from the features for future merge into final font
        // TODO do we want to do the whole blob or to emit table-by-table?
        let font = compilation
            .build_raw(&glyph_order, Default::default())
            .map_err(|_| {
                Error::FeaError(format!(
                    "{} build_raw failed; no useful diagnostic available",
                    feature_source
                ))
            })?;
        Ok(font)
    }

    fn compile_memory(
        &self,
        context: &Context,
        fea_content: &String,
        glyph_order: GlyphMap,
    ) -> Result<FontBuilder, Error> {
        // Will you not parse?!

        // TODO write out the feature content on failure
        let parse = fea_rs::parse_from_memory(fea_content, Some(&glyph_order))
            .map_err(|e| Error::FeaError(format!("{:?} parsing in-memory feature content", e)));
        if parse.is_err() {
            write_debug_fea(context, parse.is_err(), "fea parse failed", fea_content);
        }
        let parse = parse?;
        self.compile_parse("Memory", parse, glyph_order)
    }

    /// Inspired by (as in shameless copy of) how the fea-rs binary flows.
    fn compile_file(&self, fea_file: &Path, glyph_order: GlyphMap) -> Result<FontBuilder, Error> {
        // Will you not parse?!
        let parse =
            fea_rs::parse_root_file(fea_file, Some(&glyph_order), Some(self.build_dir.clone()))
                .map_err(|e| Error::FeaError(format!("{:?} parsing {:?}", e, fea_file)))?;

        self.compile_parse(fea_file.to_str().unwrap_or_default(), parse, glyph_order)
    }
}

fn write_debug_fea(context: &Context, is_error: bool, why: &str, fea_content: &String) {
    let debug_file = context.debug_dir().join("glyphs.fea");
    match fs::write(&debug_file, fea_content) {
        Ok(..) => {
            if is_error {
                warn!("{}; fea written to {:?}", why, debug_file)
            } else {
                debug!("fea written to {:?}", debug_file);
            }
        }
        Err(e) => error!("{}; failed to write fea to {:?}: {}", why, debug_file, e),
    };
}

impl Work<Context, Error> for FeatureWork {
    fn exec(&self, context: &Context) -> Result<(), Error> {
        let features = context.ir.get_features();
        if let Features::Empty = *features {
            // set a default in place so subsequent compiles skip this step
            trace!("No fea file, dull compile");
            context.set_features(FontBuilder::default());
            return Ok(());
        }
        let glyph_order = &context.ir.get_static_metadata().glyph_order;
        if glyph_order.is_empty() {
            warn!("Glyph order is empty; feature compile improbable");
        }
        let glyph_map = glyph_order
            .iter()
            .map(|n| Into::<FeaRsGlyphName>::into(n.as_str()))
            .collect();

        let font = match &*features {
            Features::File(fea_file) => self.compile_file(fea_file, glyph_map)?,
            Features::Memory(fea_content) => {
                let result = self.compile_memory(context, fea_content, glyph_map);
                if result.is_err() || context.emit_debug {
                    write_debug_fea(context, result.is_err(), "fea compile failed", fea_content);
                }
                result?
            }
            Features::Empty => unreachable!("Empty exits early"),
        };

        context.set_features(font);
        Ok(())
    }
}