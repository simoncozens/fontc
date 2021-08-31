#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Token {
    pub(crate) len: usize,
    pub(crate) kind: Kind,
}

impl Token {
    pub const EMPTY: Token = Token {
        len: 0,
        kind: Kind::Tombstone,
    };
}

/// Kinds of tokens assigned during lexing and parsing.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u16)]
pub enum Kind {
    Tombstone,  // a placeholder value
    Eof,        // the end of the input stream
    SourceFile, // scope of a file

    // a name or a keyword or any other block of non-whitespace.
    // we will frequently have to disambiguate this based on context.
    Ident,

    String,
    StringUnterminated, // an error handled at a higher level
    Number,
    Octal,
    Hex,      // an error handled at a higher level
    HexEmpty, // naked 0x
    Float,

    Whitespace,
    Comment,

    // special symbols
    Semi,
    Comma,
    Backslash,
    Hyphen,
    Eq,
    LBrace,
    RBrace,
    LSquare,
    RSquare,
    LParen,
    RParen,
    LAngle,
    RAngle,
    SingleQuote,

    NamedGlyphClass,
    Cid,

    // top-level keywords
    TableKw,
    LookupKw,
    LanguagesystemKw,
    AnchorDefKw,
    FeatureKw,
    MarkClassKw,
    AnonKw, // 'anon' and 'anonymous'

    // other keywords
    AnchorKw,
    ByKw,
    ContourpointKw,
    CursiveKw,
    DeviceKw,
    EnumKw, // 'enum' and 'enumerate'
    ExcludeDfltKw,
    FromKw,
    IgnoreKw,
    IgnoreBaseGlyphsKw,
    IgnoreLigaturesKw,
    IgnoreMarksKw,
    IncludeKw,
    IncludeDfltKw,
    LanguageKw,
    LookupflagKw,
    MarkKw,
    MarkAttachmentTypeKw,
    NameIdKw,
    NullKw,
    ParametersKw,
    PosKw, // 'pos' and 'position'
    RequiredKw,
    RightToLeftKw,
    RsubKw, // 'rsub' and 'reversesub'
    ScriptKw,
    SubKw, // 'sub' and 'substitute'
    SubtableKw,
    UseExtensionKw,
    UseMarkFilteringSetKw,
    ValueRecordDefKw,

    // keywords only in specific table contexts:
    HorizAxisBaseScriptListKw,   //BASE table
    HorizAxisBaseTagListKw,      //BASE table
    HorizAxisMinMaxKw,           //BASE table
    VertAxisBaseScriptListKw,    //BASE table
    VertAxisBaseTagListKw,       //BASE table
    VertAxisMinMaxKw,            //BASE table
    AttachKw,                    //GDEF table
    GlyphClassDefKw,             //GDEF table
    LigatureCaretByDevKw,        //GDEF table
    LigatureCaretByIndexKw,      //GDEF table
    LigatureCaretByPosKw,        //GDEF table
    MarkAttachClassKw,           //GDEF table
    FontRevisionKw,              //head table
    AscenderKw,                  //hhea table
    CaretOffsetKw,               //hhea table
    DescenderKw,                 //hhea table
    LineGapKw,                   //hhea table
    CapHeightKw,                 //OS/2 table
    CodePageRangeKw,             //OS/2 table
    PanoseKw,                    //OS/2 table
    TypoAscenderKw,              //OS/2 table
    TypoDescenderKw,             //OS/2 table
    TypoLineGapKw,               //OS/2 table
    UnicodeRangeKw,              //OS/2 table
    VendorKw,                    //OS/2 table
    WinAscentKw,                 //OS/2 table
    WinDescentKw,                //OS/2 table
    XHeightKw,                   //OS/2 table
    SizemenunameKw,              //size feature
    VertTypoAscenderKw,          //vhea table
    VertTypoDescenderKw,         //vhea table
    VertTypoLineGapKw,           //vhea table
    VertAdvanceYKw,              //vmtx table
    VertOriginYKw,               //vmtx table
    ElidedFallbackNameKw,        //STAT table
    ElidedFallbackNameIDKw,      //STAT table
    DesignAxisKw,                //STAT table
    AxisValueKw,                 //STAT table
    FlagKw,                      //STAT table
    LocationKw,                  //STAT table
    ElidableAxisValueNameKw,     //STAT table
    OlderSiblingFontAttributeKw, //STAT table

    // not technically keywords and not lexed, but assigned during parsing
    // in gsub/gpos:
    LigatureKw,
    BaseKw,

    // not lexed
    GlyphRange,
    Metric,
    Label,
    Tag,
    Path,
    GlyphName,
    GlyphClass,

    // keywords:
    // keywords only in special context, not lexed
    //HorizAxisBaseScriptList,
    //HorizAxisBaseTagList,
    //HorizAxisMinMax,
    //VertAxisBaseScriptList,
    //VertAxisBaseTagList,
    //VertAxisMinMax,

    // general purpose table node
    TableEntryNode,
    // node-only tokens, assigned during parsing
    GposNode,
    GsubNode,
    AnchorMarkNode,
    ValueRecordNode,
    LookupRefNode,
    LookupBlockNode,
    ScriptRecordNode,
}

impl Kind {
    // only used for debugging
    pub(crate) fn has_contents(&self) -> bool {
        matches!(
            self,
            Self::Ident
                | Self::String
                | Self::StringUnterminated
                | Self::Float
                | Self::Hex
                | Self::HexEmpty
                | Self::Octal
                | Self::Comment
                | Self::Whitespace
                | Self::NamedGlyphClass
                | Self::GlyphName
                | Self::Metric
                | Self::Number
                | Self::Label
        )
    }

    pub(crate) fn from_keyword(word: &[u8]) -> Option<Kind> {
        //eprintln!("{}", std::str::from_utf8(word).unwrap());
        match word {
            b"anchor" => Some(Kind::AnchorKw),
            b"anchorDef" => Some(Kind::AnchorDefKw),
            b"anon" | b"anonymous" => Some(Kind::AnonKw),
            b"by" => Some(Kind::ByKw),
            b"contourpoint" => Some(Kind::ContourpointKw),
            b"cursive" => Some(Kind::CursiveKw),
            b"device" => Some(Kind::DeviceKw), //[ Not implemented ];
            b"enum" | b"enumerate" => Some(Kind::EnumKw),
            b"exclude_dflt" | b"excludeDFLT" => Some(Kind::ExcludeDfltKw),
            b"feature" => Some(Kind::FeatureKw), //(used as a block and as a statement);
            b"from" => Some(Kind::FromKw),
            b"ignore" => Some(Kind::IgnoreKw), //(used with substitute and position);
            b"IgnoreBaseGlyphs" => Some(Kind::IgnoreBaseGlyphsKw),
            b"IgnoreLigatures" => Some(Kind::IgnoreLigaturesKw),
            b"IgnoreMarks" => Some(Kind::IgnoreMarksKw),
            b"include" => Some(Kind::IncludeKw),
            b"include_dflt" | b"includeDFLT" => Some(Kind::IncludeDfltKw),
            b"language" => Some(Kind::LanguageKw),
            b"languagesystem" => Some(Kind::LanguagesystemKw),
            b"lookup" => Some(Kind::LookupKw),
            b"lookupflag" => Some(Kind::LookupflagKw),
            b"mark" => Some(Kind::MarkKw),
            b"MarkAttachmentType" => Some(Kind::MarkAttachmentTypeKw),
            b"markClass" => Some(Kind::MarkClassKw),
            b"nameid" => Some(Kind::NameIdKw),
            b"NULL" => Some(Kind::NullKw), //(used in substitute, device, value record, anchor);
            b"parameters" => Some(Kind::ParametersKw),
            b"pos" | b"position" => Some(Kind::PosKw),
            b"required" => Some(Kind::RequiredKw), //[ Not implemented ];
            b"reversesub" | b"rsub" => Some(Kind::RsubKw),
            b"RightToLeft" => Some(Kind::RightToLeftKw),
            b"script" => Some(Kind::ScriptKw),
            b"substitute" | b"sub" => Some(Kind::SubKw),
            b"subtable" => Some(Kind::SubtableKw),
            b"table" => Some(Kind::TableKw),
            b"useExtension" => Some(Kind::UseExtensionKw),
            b"UseMarkFilteringSet" => Some(Kind::UseMarkFilteringSetKw),
            b"valueRecordDef" => Some(Kind::ValueRecordDefKw),
            b"HorizAxis.BaseScriptList" => Some(Kind::HorizAxisBaseScriptListKw),
            b"HorizAxis.BaseTagList" => Some(Kind::HorizAxisBaseTagListKw),
            b"HorizAxis.MinMax" => Some(Kind::HorizAxisMinMaxKw),
            b"VertAxis.BaseScriptList" => Some(Kind::VertAxisBaseScriptListKw),
            b"VertAxis.BaseTagList" => Some(Kind::VertAxisBaseTagListKw),
            b"VertAxis.MinMax" => Some(Kind::VertAxisMinMaxKw),
            b"Attach" => Some(Kind::AttachKw),
            b"GlyphClassDef" => Some(Kind::GlyphClassDefKw),
            b"LigatureCaretByDev" => Some(Kind::LigatureCaretByDevKw),
            b"LigatureCaretByIndex" => Some(Kind::LigatureCaretByIndexKw),
            b"LigatureCaretByPos" => Some(Kind::LigatureCaretByPosKw),
            b"MarkAttachClass" => Some(Kind::MarkAttachClassKw),
            b"FontRevision" => Some(Kind::FontRevisionKw),
            b"Ascender" => Some(Kind::AscenderKw),
            b"CaretOffset" => Some(Kind::CaretOffsetKw),
            b"Descender" => Some(Kind::DescenderKw),
            b"LineGap" => Some(Kind::LineGapKw),
            b"CapHeight" => Some(Kind::CapHeightKw),
            b"CodePageRange" => Some(Kind::CodePageRangeKw),
            b"Panose" => Some(Kind::PanoseKw),
            b"TypoAscender" => Some(Kind::TypoAscenderKw),
            b"TypoDescender" => Some(Kind::TypoDescenderKw),
            b"TypoLineGap" => Some(Kind::TypoLineGapKw),
            b"UnicodeRange" => Some(Kind::UnicodeRangeKw),
            b"Vendor" => Some(Kind::VendorKw),
            b"winAscent" => Some(Kind::WinAscentKw),
            b"winDescent" => Some(Kind::WinDescentKw),
            b"XHeight" => Some(Kind::XHeightKw),
            b"sizemenuname" => Some(Kind::SizemenunameKw),
            b"VertTypoAscender" => Some(Kind::VertTypoAscenderKw),
            b"VertTypoDescender" => Some(Kind::VertTypoDescenderKw),
            b"VertTypoLineGap" => Some(Kind::VertTypoLineGapKw),
            b"VertAdvanceY" => Some(Kind::VertAdvanceYKw),
            b"VertOriginY" => Some(Kind::VertOriginYKw),
            b"ElidedFallbackName" => Some(Kind::ElidedFallbackNameKw),
            b"ElidedFallbackNameID" => Some(Kind::ElidedFallbackNameIDKw),
            b"DesignAxis" => Some(Kind::DesignAxisKw),
            b"AxisValue" => Some(Kind::AxisValueKw),
            b"flag" => Some(Kind::FlagKw),
            b"location" => Some(Kind::LocationKw),
            b"ElidableAxisValueName" => Some(Kind::ElidableAxisValueNameKw),
            b"OlderSiblingFontAttribute" => Some(Kind::OlderSiblingFontAttributeKw),
            _ => None,
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Eof => write!(f, "EOF"),
            Self::Tombstone => write!(f, "X_X"),
            Self::SourceFile => write!(f, "FILE"),
            Self::Ident => write!(f, "ID"),
            Self::StringUnterminated => write!(f, "STR OPEN"),
            Self::String => write!(f, "STR"),
            Self::Number => write!(f, "NUM"),
            Self::Octal => write!(f, "OCT"),
            Self::Hex => write!(f, "HEX"),
            Self::HexEmpty => write!(f, "HEX EMPTY"),
            Self::Float => write!(f, "FLOAT"),
            Self::Whitespace => write!(f, "WS"),
            Self::Semi => write!(f, ";"),
            Self::Comma => write!(f, ","),
            //Self::At => write!(f, "@"),
            Self::Backslash => write!(f, "\\"),
            Self::Hyphen => write!(f, "-"), // also minus
            Self::Eq => write!(f, "="),
            Self::LBrace => write!(f, "{{"),
            Self::RBrace => write!(f, "}}"),
            Self::LSquare => write!(f, "["),
            Self::RSquare => write!(f, "]"),
            Self::LParen => write!(f, "("),
            Self::RParen => write!(f, ")"),
            Self::LAngle => write!(f, "<"),
            Self::RAngle => write!(f, ">"),
            Self::SingleQuote => write!(f, "'"),
            Self::Comment => write!(f, "#"),

            Self::Tag => write!(f, "Tag"),
            Self::Path => write!(f, "Path"),
            Self::GlyphClass => write!(f, "GlyphClass"),
            Self::NamedGlyphClass => write!(f, "@GlyphClass"),
            Self::GlyphRange => write!(f, "GlyphRange"),
            Self::GlyphName => write!(f, "GlyphName"),
            Self::Cid => write!(f, "CID"),
            Self::Metric => write!(f, "METRIC"),
            Self::Label => write!(f, "LABEL"),

            Self::TableKw => write!(f, "TableKw"),
            Self::LookupKw => write!(f, "LookupKw"),
            Self::LanguagesystemKw => write!(f, "LanguagesystemKw"),
            Self::AnchorDefKw => write!(f, "AnchorDefKw"),
            Self::FeatureKw => write!(f, "FeatureKw"),
            Self::MarkClassKw => write!(f, "MarkClassKw"),
            Self::AnonKw => write!(f, "AnonKw"),
            Self::AnchorKw => write!(f, "AnchorKw"),
            Self::ByKw => write!(f, "ByKw"),
            Self::ContourpointKw => write!(f, "ContourpointKw"),
            Self::CursiveKw => write!(f, "CursiveKw"),
            Self::DeviceKw => write!(f, "DeviceKw"),
            Self::EnumKw => write!(f, "EnumKw"),
            Self::ExcludeDfltKw => write!(f, "ExcludeDfltKw"),
            Self::FromKw => write!(f, "FromKw"),
            Self::IgnoreKw => write!(f, "IgnoreKw"),
            Self::IgnoreBaseGlyphsKw => write!(f, "IgnoreBaseGlyphsKw"),
            Self::IgnoreLigaturesKw => write!(f, "IgnoreLigaturesKw"),
            Self::IgnoreMarksKw => write!(f, "IgnoreMarksKw"),
            Self::IncludeKw => write!(f, "IncludeKw"),
            Self::IncludeDfltKw => write!(f, "IncludeDfltKw"),
            Self::LanguageKw => write!(f, "LanguageKw"),
            Self::LookupflagKw => write!(f, "LookupflagKw"),
            Self::MarkKw => write!(f, "MarkKw"),
            Self::MarkAttachmentTypeKw => write!(f, "MarkAttachmentTypeKw"),
            Self::NameIdKw => write!(f, "NameIdKw"),
            Self::NullKw => write!(f, "NullKw"),
            Self::ParametersKw => write!(f, "ParametersKw"),
            Self::PosKw => write!(f, "PosKw"),
            Self::RequiredKw => write!(f, "RequiredKw"),
            Self::RightToLeftKw => write!(f, "RightToLeftKw"),
            Self::RsubKw => write!(f, "RsubKw"),
            //Self::ReversesubKw => write!(f, "ReversesubKw"),
            Self::ScriptKw => write!(f, "ScriptKw"),
            Self::SubKw => write!(f, "SubKw"),
            Self::SubtableKw => write!(f, "SubtableKw"),
            Self::UseExtensionKw => write!(f, "UseExtensionKw"),
            Self::UseMarkFilteringSetKw => write!(f, "UseMarkFilteringSetKw"),
            Self::ValueRecordDefKw => write!(f, "ValueRecordDefKw"),
            Self::HorizAxisBaseScriptListKw => write!(f, "HorizAxis.BaseScriptList"),
            Self::HorizAxisBaseTagListKw => write!(f, "HorizAxis.BaseTagList"),
            Self::HorizAxisMinMaxKw => write!(f, "HorizAxis.MinMax"),
            Self::VertAxisBaseScriptListKw => write!(f, "VertAxis.BaseScriptList"),
            Self::VertAxisBaseTagListKw => write!(f, "VertAxis.BaseTagList"),
            Self::VertAxisMinMaxKw => write!(f, "VertAxis.MinMax"),
            Self::AttachKw => write!(f, "Attach"),
            Self::GlyphClassDefKw => write!(f, "GlyphClassDef"),
            Self::LigatureCaretByDevKw => write!(f, "LigatureCaretByDev"),
            Self::LigatureCaretByIndexKw => write!(f, "LigatureCaretByIndex"),
            Self::LigatureCaretByPosKw => write!(f, "LigatureCaretByPos"),
            Self::MarkAttachClassKw => write!(f, "MarkAttachClass"),
            Self::FontRevisionKw => write!(f, "FontRevision"),
            Self::AscenderKw => write!(f, "Ascender"),
            Self::CaretOffsetKw => write!(f, "CaretOffset"),
            Self::DescenderKw => write!(f, "Descender"),
            Self::LineGapKw => write!(f, "LineGap"),
            Self::CapHeightKw => write!(f, "CapHeight"),
            Self::CodePageRangeKw => write!(f, "CodePageRange"),
            Self::PanoseKw => write!(f, "Panose"),
            Self::TypoAscenderKw => write!(f, "TypoAscender"),
            Self::TypoDescenderKw => write!(f, "TypoDescender"),
            Self::TypoLineGapKw => write!(f, "TypoLineGap"),
            Self::UnicodeRangeKw => write!(f, "UnicodeRange"),
            Self::VendorKw => write!(f, "Vendor"),
            Self::WinAscentKw => write!(f, "winAscent"),
            Self::WinDescentKw => write!(f, "winDescent"),
            Self::XHeightKw => write!(f, "XHeight"),
            Self::SizemenunameKw => write!(f, "sizemenuname"),
            Self::VertTypoAscenderKw => write!(f, "VertTypoAscender"),
            Self::VertTypoDescenderKw => write!(f, "VertTypoDescender"),
            Self::VertTypoLineGapKw => write!(f, "VertTypoLineGap"),
            Self::VertAdvanceYKw => write!(f, "VertAdvanceY"),
            Self::VertOriginYKw => write!(f, "VertOriginY"),
            Self::ElidedFallbackNameKw => write!(f, "ElidedFallbackName"),
            Self::ElidedFallbackNameIDKw => write!(f, "ElidedFallbackNameID"),
            Self::DesignAxisKw => write!(f, "DesignAxis"),
            Self::AxisValueKw => write!(f, "AxisValue"),
            Self::FlagKw => write!(f, "flag"),
            Self::LocationKw => write!(f, "location"),
            Self::ElidableAxisValueNameKw => write!(f, "ElidableAxisValueName"),
            Self::OlderSiblingFontAttributeKw => write!(f, "OlderSiblingFontAttribute"),

            Self::LigatureKw => write!(f, "LigatureKw"),
            Self::BaseKw => write!(f, "BaseKw"),

            Self::AnchorMarkNode => write!(f, "AnchorMarkNode"),
            Self::ValueRecordNode => write!(f, "ValueRecordNode"),
            Self::GsubNode => write!(f, "GsubNode"),
            Self::GposNode => write!(f, "GposNode"),
            Self::LookupRefNode => write!(f, "LookupRefNode"),
            Self::LookupBlockNode => write!(f, "LookupBlockNode"),
            Self::ScriptRecordNode => write!(f, "ScriptRecoordNode"),
            Self::TableEntryNode => write!(f, "TableEntryNode"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 128 is the max size of our TokenSet.
    #[test]
    fn max_lexed_token_discriminent() {
        assert!(
            (Kind::OlderSiblingFontAttributeKw as u16) < 127,
            "{}",
            Kind::OlderSiblingFontAttributeKw as u16
        );
    }
}
