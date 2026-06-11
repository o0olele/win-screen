use crate::{CapturedImage, Result};

pub trait Annotation {
    fn apply(&self, image: &mut CapturedImage) -> Result<()>;
}
