mod wgpu_buffer_download;
mod wgpu_buffer_upload;
mod wgpu_sobel_buf;
mod wgpu_sobel_mem;
mod wgpu_texture_copy;
mod wgpu_texture_download;
mod wgpu_texture_upload;

extern crate gstreamer as gst;
extern crate gstreamer_base as gst_base;
extern crate gstreamer_video as gst_video;

use gst::glib;

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    wgpu_sobel_mem::register(plugin)?;
    wgpu_buffer_upload::register(plugin)?;
    wgpu_buffer_download::register(plugin)?;
    wgpu_sobel_buf::register(plugin)?;
    wgpu_texture_upload::register(plugin)?;
    wgpu_texture_copy::register(plugin)?;
    wgpu_texture_download::register(plugin)?;
    Ok(())
}

gst::plugin_define!(
    deka_wgpu_image_processing_rs,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    "MIT/X11",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);
