// Use `pub(crate) use <macro_name>` to make a macro importable from elsewhere.
// See https://stackoverflow.com/a/31749071

macro_rules! id_alias {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
        pub struct $name(Id);

        impl $name {
            pub fn of(str: &str) -> Self {
                Self(Id::of(str))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

pub(crate) use id_alias;

macro_rules! packets {
    (
        $( cmd $cmdName:ident($cmd:ident, $rpl:ident), )* // Commands with reply
        $( ntf $ntfName:ident($ntf:ident), )* // Notifications
    ) => {
        #[derive(Debug, Deserialize, Serialize)]
        #[serde(tag = "name", content = "data")]
        pub enum Cmd {
            $( $cmdName($cmd), )*
        }

        $(
            impl std::convert::TryFrom<Cmd> for $cmd {
                type Error = ();
                fn try_from(cmd: Cmd) -> Result<Self, Self::Error> {
                    match cmd {
                        Cmd::$cmdName(val) => Ok(val),
                        _ => Err(()),
                    }
                }
            }

            impl From<$cmd> for Cmd {
                fn from(cmd: $cmd) -> Self {
                    Self::$cmdName(cmd)
                }
            }
        )*

        #[derive(Debug, Deserialize, Serialize)]
        #[serde(tag = "name", content = "data")]
        pub enum Rpl {
            $( $cmdName($rpl), )*
        }

        $(
            impl std::convert::TryFrom<Rpl> for $rpl {
                type Error = ();
                fn try_from(rpl: Rpl) -> Result<Self, Self::Error> {
                    match rpl {
                        Rpl::$cmdName(val) => Ok(val),
                        _ => Err(()),
                    }
                }
            }

            impl From<$rpl> for Rpl {
                fn from(rpl: $rpl) -> Self {
                    Self::$cmdName(rpl)
                }
            }
        )*

        #[derive(Debug, Deserialize, Serialize)]
        #[serde(tag = "name", content = "data")]
        pub enum Ntf {
            $( $ntfName($ntf), )*
        }

        $(
            impl std::convert::TryFrom<Ntf> for $ntf {
                type Error = ();
                fn try_from(ntf: Ntf) -> Result<Self, Self::Error> {
                    match ntf {
                        Ntf::$ntfName(val) => Ok(val),
                        _ => Err(()),
                    }
                }
            }

            impl From<$ntf> for Ntf {
                fn from(ntf: $ntf) -> Self {
                    Self::$ntfName(ntf)
                }
            }
        )*
    };
}

pub(crate) use packets;