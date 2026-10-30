/// Implements `as_str`, `Display` and `FromStr` (case-insensitive) for a fieldless enum.
macro_rules! text_enum {
    ($name:ident { $($var:ident => $text:literal),+ $(,)? }) => {
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$var),+];

            pub fn as_str(self) -> &'static str {
                match self { $($name::$var => $text),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> std::result::Result<Self, String> {
                match s.trim().to_lowercase().as_str() {
                    $($text => Ok($name::$var),)+
                    other => Err(format!(
                        "unknown value \"{other}\"; expected one of: {}",
                        [$($text),+].join(", ")
                    )),
                }
            }
        }
    };
}
