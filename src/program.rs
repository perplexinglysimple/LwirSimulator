use crate::bundle::Bundle;
use crate::layout::ProcessorLayout;

#[derive(Clone, Debug)]
pub struct Program {
    pub layout: ProcessorLayout,
    pub bundles: Vec<Bundle>,
}

impl std::ops::Deref for Program {
    type Target = Vec<Bundle>;

    fn deref(&self) -> &Self::Target {
        &self.bundles
    }
}
