mod imp;

use gst::glib;
use gst::prelude::*;

glib::wrapper! {

    /// Plugin that upload buffer to GPU
    pub struct WgpuBufferDownload(ObjectSubclass<imp::WgpuBufferDownload>) @extends gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "dekawgpubufferdownload",
        gst::Rank::NONE,
        WgpuBufferDownload::static_type(),
    )
}
