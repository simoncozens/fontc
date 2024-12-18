use darling::FromField;

#[derive(Clone, Debug, FromField)]
#[darling(attributes(fromplist, toplist))]
pub(crate) struct FieldAttrs {
    pub(crate) ignore: Option<bool>,
    pub(crate) other: Option<bool>,
    pub(crate) always_serialize: Option<bool>,
    #[darling(rename = "key")]
    pub(crate) plist_field_name: Option<String>,
    #[darling(multiple, rename = "alt_name")]
    pub(crate) plist_addtl_names: Vec<String>,
    pub(crate) filter: Option<String>,
}
