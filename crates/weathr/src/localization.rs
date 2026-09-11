use std::sync::Arc;

/// Host-owned formatting against an immutable language snapshot. Weathr passes
/// message identifiers and owned named arguments; it never loads locale files.
pub type LocalizationProvider = Arc<dyn Fn(&str, &[(&str, String)]) -> String + Send + Sync>;

macro_rules! localize {
    ($provider:expr, $id:expr $(, $name:ident = $value:expr)* $(,)?) => {
        ($provider)($id, &[$((stringify!($name), ($value).to_string())),*])
    };
}
pub(crate) use localize;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // A host mock, not a resource reader: tests remain inside Weathr's dependency
    // boundary and exercise only the provider contract and text layout.
    pub(crate) fn english() -> LocalizationProvider {
        provider(false)
    }

    pub(crate) fn chinese() -> LocalizationProvider {
        provider(true)
    }

    fn provider(chinese: bool) -> LocalizationProvider {
        Arc::new(move |id, args| {
            let arg = |name| {
                args.iter()
                    .find(|(key, _)| *key == name)
                    .unwrap()
                    .1
                    .as_str()
            };
            match id {
                "weathr-quit-prompt" => if chinese {
                    "按空格键退出"
                } else {
                    "Press Space to quit"
                }
                .into(),
                "weathr-start-prompt" => if chinese {
                    "按空格键开始"
                } else {
                    "Press Space to start"
                }
                .into(),
                "weathr-condition-clear" => if chinese { "晴" } else { "Clear" }.into(),
                "weathr-loading" => if chinese { "加载中" } else { "Loading" }.into(),
                "weathr-north" => if chinese { "北纬" } else { "N" }.into(),
                "weathr-south" => if chinese { "南纬" } else { "S" }.into(),
                "weathr-east" => if chinese { "东经" } else { "E" }.into(),
                "weathr-west" => if chinese { "西经" } else { "W" }.into(),
                "weathr-coordinates" => {
                    if chinese {
                        format!(
                            "{}{}°，{}{}°",
                            arg("latitude_direction"),
                            arg("latitude"),
                            arg("longitude_direction"),
                            arg("longitude")
                        )
                    } else {
                        format!(
                            "{}°{}, {}°{}",
                            arg("latitude"),
                            arg("latitude_direction"),
                            arg("longitude"),
                            arg("longitude_direction")
                        )
                    }
                }
                "weathr-city-coordinates" => {
                    if chinese {
                        format!("{}（{}）", arg("city"), arg("coordinates"))
                    } else {
                        format!("{} ({})", arg("city"), arg("coordinates"))
                    }
                }
                "weathr-location" => format!(
                    "{}{}",
                    if chinese { "位置：" } else { "Location: " },
                    arg("location")
                ),
                "weathr-hud-location" => format!("{} | {}", arg("location"), arg("prompt")),
                "weathr-hud-offline" => format!(
                    "{} | {}",
                    if chinese { "离线" } else { "OFFLINE" },
                    arg("content")
                ),
                "weathr-summary" => format!(
                    "{}  {}{}",
                    arg("condition"),
                    arg("temperature"),
                    arg("unit")
                ),
                _ => panic!("unexpected mock localization message: {id}"),
            }
        })
    }
}
