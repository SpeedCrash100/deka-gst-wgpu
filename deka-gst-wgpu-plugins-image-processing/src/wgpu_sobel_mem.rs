mod imp;

use gst::glib;
use gst::prelude::*;

glib::wrapper! {
    pub struct WgpuSobelMem(ObjectSubclass<imp::WgpuSobelMem>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "dekawgpusobelmem",
        gst::Rank::NONE,
        WgpuSobelMem::static_type(),
    )
}
