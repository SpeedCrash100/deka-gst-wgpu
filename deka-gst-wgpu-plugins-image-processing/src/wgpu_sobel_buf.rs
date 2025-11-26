mod imp;

use gst::glib;
use gst::prelude::*;

glib::wrapper! {

    /// Plugin that apply Sobel kernel to image
    pub struct WgpuSobelBuf(ObjectSubclass<imp::WgpuSobelBuf>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "dekawgpusobelbuf",
        gst::Rank::NONE,
        WgpuSobelBuf::static_type(),
    )
}
