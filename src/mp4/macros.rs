macro_rules! alias_strict {
    ($alias:ident, $core:ident, $ret:ty) => {
        pub fn $alias(data: &[u8]) -> crate::errors::MediaParserResult<$ret> {
            $core(data).map_err(|e| {
                crate::errors::MediaParserError::Mp4(crate::errors::Mp4Error::Error {
                    message: e.to_string(),
                })
            })
        }
    };
}

macro_rules! alias_lenient {
    ($alias:ident, $core:ident, $ret:ty) => {
        pub fn $alias(data: &[u8]) -> $ret {
            $core(data).unwrap_or_default()
        }
    };
}
