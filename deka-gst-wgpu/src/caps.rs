//!
//! Helpers to work with caps
//!

/// Creates copy of caps where each structure copied with all buffer usages from `usages`
pub fn make_wgpu_buffer_usages_for_caps<F, I>(input: &gst::Caps, usages: F) -> gst::Caps
where
    F: Fn() -> I,
    I: IntoIterator<Item = wgpu::BufferUsages>,
{
    let mut caps_builder = gst::Caps::builder_full();
    let mem_feature =
        gst::CapsFeatures::new([crate::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER]);

    for s in input.iter() {
        caps_builder = usages().into_iter().map(|usages| usages.bits()).fold(
            caps_builder,
            |caps_builder, bits| {
                let mut s_owned = s.to_owned();
                s_owned.set(crate::buffer_memory::GST_CAPS_FIELD_WGPU_BUFFER_USAGE, bits);
                caps_builder.structure_with_features(s_owned, mem_feature.clone())
            },
        )
    }

    caps_builder.build()
}
