use gst::glib;
use gst::prelude::*;

glib::wrapper! {

    /// Plugin that copies tetxure to texture
    ///  gst-launch-1.0 filesrc location=/home/deucalion/Movies/yokohama_10min.mkv ! decodebin  ! videoconvert  ! dekawgpubufferupload ! dekawgputextureupload ! dekawgputexturecopy ! dekawgputexturedownload ! dekawgpubufferdownload ! videoconvert ! autovideosink
    pub struct WgpuTextureCopy(ObjectSubclass<imp::WgpuTextureCopy>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "dekawgputexturecopy",
        gst::Rank::NONE,
        WgpuTextureCopy::static_type(),
    )
}

mod imp {
    use std::sync::LazyLock;

    use crate::glib;

    use deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER;

    use deka_gst_wgpu::texture_memory::{
        WgpuTextureMemory, WgpuTextureMemoryAllocator, WgpuTextureMemoryExt,
        GST_CAPS_FEATURE_MEMORY_WGPU_TEXTURE, GST_CAPS_FIELD_WGPU_TEXTURE_USAGE,
    };
    use glib::object::Cast;
    use glib::subclass::{object::ObjectImpl, types::ObjectSubclass};
    use gst::prelude::ElementExt;
    use gst::subclass::prelude::*;
    use gst_base::subclass::prelude::*;
    use gst_base::subclass::BaseTransformMode;
    use gst_video::prelude::*;
    use gst_video::subclass::prelude::VideoFilterImpl;
    use parking_lot::Mutex;

    use deka_gst_wgpu::{WgpuContext, GST_CONTEXT_WGPU_TYPE};

    static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
        gst::DebugCategory::new(
            "dekawgputexturecopy",
            gst::DebugColorFlags::empty(),
            Some("Deka's WebGPU texture to texture copy"),
        )
    });

    #[derive(Debug)]
    pub struct WgpuTextureCopy {
        wgpu_context: Mutex<Option<WgpuContext>>,

        sink_usages: Mutex<wgpu::TextureUsages>,
        src_usages: Mutex<wgpu::TextureUsages>,
    }

    impl WgpuTextureCopy {
        pub fn set_wgpu_context(&self, context: WgpuContext) {
            let mut lock: parking_lot::lock_api::MutexGuard<
                '_,
                parking_lot::RawMutex,
                Option<WgpuContext>,
            > = self.wgpu_context.lock();

            if lock.is_some() {
                return;
            }

            *lock = Some(context);
        }

        fn create_own_context(&self) {
            gst::info!(CAT, imp: self, "creating own wgpu context");

            let obj = self.obj();
            let element = obj.upcast_ref::<gst::Element>();

            let wgpu_ctx = WgpuContext::default();
            let ctx = wgpu_ctx.as_gst_context();
            self.set_context(&ctx);

            let message = gst::message::HaveContext::builder(ctx)
                .src(&*self.obj())
                .build();
            element.post_message(message).unwrap();
        }

        /// Locks context
        fn locked_context(&self) -> parking_lot::MappedMutexGuard<'_, WgpuContext> {
            parking_lot::MutexGuard::map(self.wgpu_context.lock(), |x| x.as_mut().unwrap())
        }

        fn sink_allowed_usages() -> impl IntoIterator<Item = wgpu::TextureUsages> {
            // We need to be able to copy from buffer
            [
                wgpu::TextureUsages::COPY_SRC,
                wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            ]
        }

        fn src_allowed_usages() -> impl IntoIterator<Item = wgpu::TextureUsages> {
            // We want to copy into the texture
            [
                wgpu::TextureUsages::COPY_DST,
                wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            ]
        }

        fn allowed_texture_formats_as_gst() -> impl IntoIterator<Item = gst_video::VideoFormat> {
            [gst_video::VideoFormat::Rgba, gst_video::VideoFormat::Rgbx]
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WgpuTextureCopy {
        const NAME: &'static str = "GstWgpuTextureCopy";
        type Type = super::WgpuTextureCopy;
        type ParentType = gst_video::VideoFilter;

        fn with_class(_klass: &Self::Class) -> Self {
            Self {
                wgpu_context: Mutex::new(None),
                src_usages: Mutex::new(wgpu::TextureUsages::empty()),
                sink_usages: Mutex::new(wgpu::TextureUsages::empty()),
            }
        }
    }

    impl ObjectImpl for WgpuTextureCopy {}
    impl GstObjectImpl for WgpuTextureCopy {}
    impl ElementImpl for WgpuTextureCopy {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static ELEMENT_METADATA: LazyLock<gst::subclass::ElementMetadata> =
                LazyLock::new(|| {
                    gst::subclass::ElementMetadata::new(
                        "Deka's WebGPU Texture to texture copy",
                        "Filter/Effect/Video",
                        "Copies between textures",
                        "Deka <speedcrash100@ya.ru>",
                    )
                });
            Some(&*ELEMENT_METADATA)
        }

        fn pad_templates() -> &'static [gst::PadTemplate] {
            static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
                // TODO: we need to figure out allowed format by allowed texture usages
                // see wgpu-info

                let def_ctx = WgpuContext::default();
                let limits = def_ctx.limits();

                let base_sink_caps = gst_video::VideoCapsBuilder::new()
                    .format_list(WgpuTextureCopy::allowed_texture_formats_as_gst())
                    .height_range(1..limits.max_texture_dimension_2d as i32)
                    .width_range(1..limits.max_texture_dimension_2d as i32)
                    .features([GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER])
                    .build();

                let base_src_caps = gst_video::VideoCapsBuilder::new()
                    .format_list(WgpuTextureCopy::allowed_texture_formats_as_gst())
                    .height_range(1..limits.max_texture_dimension_2d as i32)
                    .width_range(1..limits.max_texture_dimension_2d as i32)
                    .features([GST_CAPS_FEATURE_MEMORY_WGPU_TEXTURE])
                    .build();

                let sink_caps = deka_gst_wgpu::caps::transform::gst_caps_with_texture_usages(
                    base_sink_caps,
                    WgpuTextureCopy::sink_allowed_usages,
                );

                let src_caps = deka_gst_wgpu::caps::transform::gst_caps_with_texture_usages(
                    base_src_caps,
                    WgpuTextureCopy::src_allowed_usages,
                );

                vec![
                    gst::PadTemplate::new(
                        "sink",
                        gst::PadDirection::Sink,
                        gst::PadPresence::Always,
                        &sink_caps,
                    )
                    .unwrap(),
                    gst::PadTemplate::new(
                        "src",
                        gst::PadDirection::Src,
                        gst::PadPresence::Always,
                        &src_caps,
                    )
                    .unwrap(),
                ]
            });
            PAD_TEMPLATES.as_ref()
        }

        fn set_context(&self, context: &gst::Context) {
            if context.context_type() == GST_CONTEXT_WGPU_TYPE {
                gst::debug!(CAT, imp: self, "Received wgpu context");

                let Some(wgpu_ctx) = WgpuContext::map_gst_context_to_wgpu(context.clone()) else {
                    gst::error!(CAT, imp: self, "Received invalid wgpu context");
                    return;
                };

                self.set_wgpu_context(wgpu_ctx);
            }

            self.parent_set_context(context);
        }
    }

    impl BaseTransformImpl for WgpuTextureCopy {
        const MODE: BaseTransformMode = BaseTransformMode::NeverInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;

        fn start(&self) -> Result<(), gst::ErrorMessage> {
            let obj = self.obj();
            let element = obj.upcast_ref::<gst::Element>();

            match WgpuContext::query_context_from_nearby_elements(element) {
                Ok(true) => {
                    gst::info!(CAT, imp: self, "using shared wgpu context");
                    Ok(())
                }
                Ok(false) => {
                    self.create_own_context();
                    Ok(())
                }
                Err(err) => {
                    gst::error!(CAT, imp: self, "failed to query wgpu context from nearby elements: {}", err);
                    self.create_own_context();
                    Ok(())
                }
            }
        }

        fn transform_caps(
            &self,
            direction: gst::PadDirection,
            caps: &gst::Caps,
            filter: Option<&gst::Caps>,
        ) -> Option<gst::Caps> {
            let other_caps = if direction == gst::PadDirection::Src {
                deka_gst_wgpu::caps::transform::gst_caps_with_texture_usages(
                    caps,
                    Self::sink_allowed_usages,
                )
            } else {
                deka_gst_wgpu::caps::transform::gst_caps_with_texture_usages(
                    caps,
                    Self::src_allowed_usages,
                )
            };

            gst::trace!(
                CAT,
                imp: self,
                "Transformed caps from {} to {} in direction {:?}; filter: {:?}",
                caps,
                other_caps,
                direction,
                filter
            );

            // In the end we need to filter the caps through an optional filter caps to get rid of any
            // unwanted caps.
            if let Some(filter) = filter {
                Some(filter.intersect_with_mode(&other_caps, gst::CapsIntersectMode::First))
            } else {
                Some(other_caps)
            }
        }

        fn set_caps(
            &self,
            incaps: &gst::Caps,
            outcaps: &gst::Caps,
        ) -> Result<(), gst::LoggableError> {
            gst::info!(CAT, imp: self, "negotiated caps {:?} -> {:?}", incaps, outcaps);

            {
                let Some(outcaps_s) = outcaps.structure(0) else {
                    return Err(gst::loggable_error!(
                        CAT,
                        "missing structure in output caps"
                    ));
                };

                let src_usages_bits: u32 = match outcaps_s.get(GST_CAPS_FIELD_WGPU_TEXTURE_USAGE) {
                    Ok(usage) => usage,
                    Err(err) => {
                        return Err(gst::loggable_error!(
                            CAT,
                            "cannot get texture usage in output caps: {}",
                            err
                        ));
                    }
                };
                let src_usages = wgpu::TextureUsages::from_bits_truncate(src_usages_bits);
                if !src_usages.intersects(wgpu::TextureUsages::COPY_DST) {
                    return Err(gst::loggable_error!(
                        CAT,
                        "texture usage({:?}) in output caps cannot be used as copy destination",
                        src_usages
                    ));
                }

                *self.src_usages.lock() = src_usages;
            }

            {
                let Some(incaps_s) = incaps.structure(0) else {
                    return Err(gst::loggable_error!(CAT, "missing structure in input caps"));
                };

                let sink_usages_bits: u32 = match incaps_s.get(GST_CAPS_FIELD_WGPU_TEXTURE_USAGE) {
                    Ok(usage) => usage,
                    Err(err) => {
                        return Err(gst::loggable_error!(
                            CAT,
                            "cannot get texture usage in input caps: {}",
                            err
                        ));
                    }
                };
                let sink_usages = wgpu::TextureUsages::from_bits_truncate(sink_usages_bits);
                if !sink_usages.intersects(wgpu::TextureUsages::COPY_SRC) {
                    return Err(gst::loggable_error!(
                        CAT,
                        "texture usage({:?}) in input caps cannot be used as copy source",
                        sink_usages
                    ));
                }

                *self.sink_usages.lock() = sink_usages;
            }

            self.parent_set_caps(incaps, outcaps)
        }

        fn transform(
            &self,
            inbuf: &gst::Buffer,
            outbuf: &mut gst::BufferRef,
        ) -> Result<gst::FlowSuccess, gst::FlowError> {
            assert!(0 < inbuf.n_memory());
            assert!(0 < outbuf.n_memory());
            // If we are here, we are going to copy to output memory

            let inmem = inbuf.peek_memory(0);
            let Some(inmem) = inmem.downcast_memory_ref::<WgpuTextureMemory>() else {
                gst::error!(CAT, imp: self, "invalid input memory");
                return Err(gst::FlowError::NotNegotiated);
            };

            let outmem = outbuf.peek_memory(0);
            let Some(outmem) = outmem.downcast_memory_ref::<WgpuTextureMemory>() else {
                gst::error!(CAT, imp: self, "invalid output memory");
                return Err(gst::FlowError::NotNegotiated);
            };

            let obj = self.obj();
            let self_as_filter = obj.upcast_ref::<gst_video::VideoFilter>();
            let Some(in_info) = self_as_filter.input_video_info() else {
                return Err(gst::FlowError::NotNegotiated);
            };

            {
                let src = inmem.texture();
                let dst = outmem.texture();
                let ctx = self.locked_context();
                let mut encoder = ctx.device().create_command_encoder(&Default::default());
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: src,
                        aspect: wgpu::TextureAspect::All,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: dst,
                        aspect: wgpu::TextureAspect::All,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                    },
                    wgpu::Extent3d {
                        width: in_info.width(),
                        height: in_info.height(),
                        depth_or_array_layers: 1,
                    },
                );

                ctx.queue().submit([encoder.finish()]);
            }

            Ok(gst::FlowSuccess::Ok)
        }

        fn decide_allocation(
            &self,
            query: &mut gst::query::Allocation,
        ) -> Result<(), gst::LoggableError> {
            let src_usages = self.src_usages.lock();
            if src_usages.is_empty() {
                return Err(gst::loggable_error!(
                    CAT,
                    "decide_allocation called before negotiation"
                ));
            }

            let mut to_remove = vec![];

            for (pos, (allocator, _params)) in query.allocation_params().iter().enumerate() {
                let Some(wgpu_allocator) =
                    allocator.and_downcast_ref::<WgpuTextureMemoryAllocator>()
                else {
                    gst::trace!(CAT, imp: self, "skipping allocator at {pos}, not an WGPU texture");
                    to_remove.push(pos);
                    continue;
                };

                let usages = wgpu_allocator.descriptor().usage;
                let required = wgpu::TextureUsages::COPY_DST;
                if !usages.contains(required) {
                    gst::trace!(CAT, imp: self, "skipping allocator at {pos}, usages is incorrect {} != {}", required.bits(), usages.bits());
                    to_remove.push(pos);
                }
            }

            for pos in to_remove.iter().rev() {
                query.remove_nth_allocation_param(*pos as u32);
            }

            if 0 < query.allocation_params().len() {
                return Ok(());
            }

            let (caps, _needs_pool) = query.get();

            let Some(caps) = caps else {
                return Err(gst::loggable_error!(
                    CAT,
                    "decide_allocation called wo caps"
                ));
            };

            let Some(s) = caps.structure(0) else {
                return Err(gst::loggable_error!(CAT, "caps structure missing"));
            };

            let width: i32 = match s.get("width") {
                Ok(v) => v,
                Err(err) => {
                    return Err(gst::loggable_error!(CAT, "can't find width: {}", err));
                }
            };

            let height: i32 = match s.get("height") {
                Ok(v) => v,
                Err(err) => {
                    return Err(gst::loggable_error!(CAT, "can't find width: {}", err));
                }
            };

            let desciptor = wgpu::TextureDescriptor {
                label: None,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm, // FIXME< Should be gotten from caps
                mip_level_count: 1,
                sample_count: 1,
                size: wgpu::Extent3d {
                    width: width as u32,
                    height: height as u32,
                    depth_or_array_layers: 1,
                },
                usage: *src_usages,
                view_formats: &[],
            };

            let ctx = self.wgpu_context.lock().as_ref().cloned().unwrap();
            let allocator = WgpuTextureMemoryAllocator::new(ctx, desciptor);
            let params = gst::AllocationParams::new(gst::MemoryFlags::NOT_MAPPABLE, 0, 0, 0);
            query.add_allocation_param(Some(&allocator), params);

            // No pool support at the moment
            while !query.allocation_pools().is_empty() {
                query.remove_nth_allocation_pool(0);
            }

            Ok(())
        }
    }

    impl VideoFilterImpl for WgpuTextureCopy {}
}
