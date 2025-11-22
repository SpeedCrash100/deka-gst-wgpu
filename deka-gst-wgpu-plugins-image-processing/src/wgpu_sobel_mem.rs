mod imp;

use gst::glib;
use gst::prelude::*;

glib::wrapper! {

    /// Plugin that apply Sobel kernel to image
    ///
    /// # Sample pipeline
    /// ```bash
    /// gst-launch-1.0 filesrc location=video.mkv ! decodebin ! videoconvert ! queue ! dekawgpusobelmem ! videoconvert ! autovideosink
    /// ```
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
