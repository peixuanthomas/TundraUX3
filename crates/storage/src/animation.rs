use serde::{Deserialize, Serialize};

pub const MIN_ANIMATION_PHASE_MS: u16 = 1;
pub const MAX_ANIMATION_PHASE_MS: u16 = 2_000;

/// Acceleration, cruise and deceleration milliseconds for each surface type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AnimationTimingsConfig {
    pub page: [u16; 3],
    pub dialog: [u16; 3],
    pub menu: [u16; 3],
}

impl AnimationTimingsConfig {
    pub const DEFAULT: Self = Self {
        page: [55, 110, 55],
        dialog: [45, 90, 45],
        menu: [40, 80, 40],
    };

    pub const fn is_default(&self) -> bool {
        matches!(
            self,
            Self {
                page: [55, 110, 55],
                dialog: [45, 90, 45],
                menu: [40, 80, 40]
            }
        )
    }

    pub fn normalized(self) -> Self {
        let normalize = |phases: [u16; 3]| {
            phases.map(|ms| ms.clamp(MIN_ANIMATION_PHASE_MS, MAX_ANIMATION_PHASE_MS))
        };
        Self {
            page: normalize(self.page),
            dialog: normalize(self.dialog),
            menu: normalize(self.menu),
        }
    }
}

impl Default for AnimationTimingsConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

pub(crate) fn deserialize_timings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<AnimationTimingsConfig, D::Error> {
    AnimationTimingsConfig::deserialize(deserializer).map(AnimationTimingsConfig::normalized)
}
