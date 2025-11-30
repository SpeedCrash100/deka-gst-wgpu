use crate::{
    buffer_memory::{GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER, GST_CAPS_FIELD_WGPU_BUFFER_USAGE},
    texture_memory::{GST_CAPS_FEATURE_MEMORY_WGPU_TEXTURE, GST_CAPS_FIELD_WGPU_TEXTURE_USAGE},
};

fn remove_wgpu_buffer_fields(s: &mut gst::Structure) {
    s.remove_field(GST_CAPS_FIELD_WGPU_BUFFER_USAGE);
}

fn remove_wgpu_texture_fields(s: &mut gst::Structure) {
    s.remove_field(GST_CAPS_FIELD_WGPU_TEXTURE_USAGE);
}

/// Create same caps but for texture usages
///
/// # Note
/// if caps haves WGPU related fields they will bre removed
pub fn gst_caps_with_texture_usages<C, F, I>(caps: C, usages_factory: F) -> gst::Caps
where
    C: AsRef<gst::CapsRef>,
    F: Fn() -> I,
    I: IntoIterator<Item = wgpu::TextureUsages>,
{
    let original_caps = caps.as_ref();
    let mut builder = gst::Caps::builder_full();
    let feature = gst::CapsFeatures::new([GST_CAPS_FEATURE_MEMORY_WGPU_TEXTURE]);

    for s in original_caps.iter() {
        builder = usages_factory().into_iter().map(|usage| usage.bits()).fold(
            builder,
            |builder, bits| {
                let mut new_s = s.to_owned();
                remove_wgpu_buffer_fields(&mut new_s);
                new_s.set(GST_CAPS_FIELD_WGPU_TEXTURE_USAGE, bits);
                builder.structure_with_features(new_s, feature.clone())
            },
        );
    }

    builder.build()
}

pub fn gst_caps_with_buffer_usages<C, F, I>(caps: C, usages_factory: F) -> gst::Caps
where
    C: AsRef<gst::CapsRef>,
    F: Fn() -> I,
    I: IntoIterator<Item = wgpu::BufferUsages>,
{
    let original_caps = caps.as_ref();
    let mut builder = gst::Caps::builder_full();
    let feature = gst::CapsFeatures::new([GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER]);

    for s in original_caps.iter() {
        builder = usages_factory().into_iter().map(|usage| usage.bits()).fold(
            builder,
            |builder, bits| {
                let mut new_s = s.to_owned();
                remove_wgpu_texture_fields(&mut new_s);
                new_s.set(GST_CAPS_FIELD_WGPU_BUFFER_USAGE, bits);
                builder.structure_with_features(new_s, feature.clone())
            },
        );
    }

    builder.build()
}
