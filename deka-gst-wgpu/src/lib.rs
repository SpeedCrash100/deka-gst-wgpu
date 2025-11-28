pub mod buffer_memory;
pub mod caps;
pub mod context;
pub mod texture_meta;

use gst::glib;
extern crate gstreamer as gst;
extern crate gstreamer_base as gst_base;
extern crate gstreamer_video as gst_video;

macro_rules! skip_assert_initialized {
    () => {};
}

use skip_assert_initialized;

pub mod prelude {
    use super::*;

    pub use buffer_memory::WgpuBufferMemoryExt;
}

pub use buffer_memory::{WgpuBufferMemory, WgpuBufferMemoryAllocator};
pub use context::{PollType, WgpuContext, GST_CONTEXT_WGPU_TYPE};
