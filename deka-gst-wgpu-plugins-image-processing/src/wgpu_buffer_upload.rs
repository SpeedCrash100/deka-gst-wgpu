mod imp;

use gst::glib;
use gst::prelude::*;

glib::wrapper! {

    /// Plugin that upload buffer to GPU
    pub struct WgpuBufferUpload(ObjectSubclass<imp::WgpuBufferUpload>) @extends gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "dekawgpubufferupload",
        gst::Rank::NONE,
        WgpuBufferUpload::static_type(),
    )
}
