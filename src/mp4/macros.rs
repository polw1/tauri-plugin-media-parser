macro_rules! alias_strict {
    ($alias:ident, $core:ident, $ret:ty) => {
        pub fn $alias(data: &[u8]) -> std::io::Result<$ret> {
            $core(data)
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
