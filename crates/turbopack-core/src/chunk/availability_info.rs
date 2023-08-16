use super::available_assets::AvailableAssetsVc;

#[turbo_tasks::value(serialization = "auto_for_input")]
#[derive(PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub enum AvailabilityInfo {
    Untracked,
    Root,
    Inner { available_assets: AvailableAssetsVc },
}

impl AvailabilityInfo {
    pub fn is_tracked(&self) -> bool {
        match self {
            Self::Untracked => false,
            Self::Root => true,
            Self::Inner { .. } => true,
        }
    }

    pub fn available_assets(&self) -> Option<AvailableAssetsVc> {
        match self {
            Self::Untracked => None,
            Self::Root { .. } => None,
            Self::Inner { available_assets } => Some(*available_assets),
        }
    }
}
