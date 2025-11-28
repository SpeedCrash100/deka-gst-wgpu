use std::sync::LazyLock;

use crate::glib;

use deka_gst_wgpu::buffer_memory::{WgpuBufferMemory, GST_CAPS_FIELD_WGPU_BUFFER_USAGE};

use deka_gst_wgpu::{prelude::*, WgpuBufferMemoryAllocator};
use glib::object::Cast;
use glib::subclass::{object::ObjectImpl, types::ObjectSubclass};
use gst::prelude::ElementExt;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::{BaseTransformImpl, BaseTransformImplExt};
use gst_base::subclass::BaseTransformMode;
use gst_video::prelude::*;
use parking_lot::Mutex;

use deka_gst_wgpu::{WgpuContext, GST_CONTEXT_WGPU_TYPE};

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "dekawgpubufferdownload",
        gst::DebugColorFlags::empty(),
        Some("Deka's WebGPU download from GPU buffer plugin"),
    )
});

#[derive(Debug)]
pub struct WgpuBufferDownload {
    wgpu_context: Mutex<Option<WgpuContext>>,
    sink_usages: Mutex<wgpu::BufferUsages>,
}

impl WgpuBufferDownload {
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

    fn sink_allowed_usages() -> impl IntoIterator<Item = wgpu::BufferUsages> {
        [
            wgpu::BufferUsages::MAP_WRITE,
            wgpu::BufferUsages::MAP_READ,
            wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        ]
    }
}

#[glib::object_subclass]
impl ObjectSubclass for WgpuBufferDownload {
    const NAME: &'static str = "GstWgpuBufferDownload";
    type Type = super::WgpuBufferDownload;
    type ParentType = gst_base::BaseTransform;

    fn with_class(_klass: &Self::Class) -> Self {
        Self {
            wgpu_context: Mutex::new(None),
            sink_usages: Mutex::new(wgpu::BufferUsages::empty()),
        }
    }
}

impl ObjectImpl for WgpuBufferDownload {}
impl GstObjectImpl for WgpuBufferDownload {}
impl ElementImpl for WgpuBufferDownload {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "Deka's WebGPU Buffer Download plugin",
                "Filter/Effect/Video",
                "Download buffer from GPU using WebGPU",
                "Deka <speedcrash100@ya.ru>",
            )
        });
        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
            let mem_feature = gst::CapsFeatures::new([
                deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER,
            ]);

            let sink_caps_builder = WgpuBufferDownload::sink_allowed_usages()
                .into_iter()
                .map(|usage| usage.bits())
                .fold(gst::Caps::builder_full(), |builder, item| {
                    builder
                        .structure_with_features(
                            gst::Structure::builder("audio/x-raw")
                                .field(GST_CAPS_FIELD_WGPU_BUFFER_USAGE, item)
                                .build(),
                            mem_feature.clone(),
                        )
                        .structure_with_features(
                            gst::Structure::builder("video/x-raw")
                                .field(GST_CAPS_FIELD_WGPU_BUFFER_USAGE, item)
                                .build(),
                            mem_feature.clone(),
                        )
                });

            let sink_caps = sink_caps_builder.build();
            let src_caps = gst::Caps::builder_full()
                .structure(gst::Structure::new_empty("audio/x-raw"))
                .structure(gst::Structure::new_empty("video/x-raw"))
                .build();

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

impl BaseTransformImpl for WgpuBufferDownload {
    const MODE: BaseTransformMode = BaseTransformMode::Both;
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
        let other_caps = if direction == gst::PadDirection::Sink {
            let mut builder = gst::Caps::builder_full();

            for s in caps.iter() {
                let mut new_s = s.to_owned();
                new_s.remove_field(deka_gst_wgpu::buffer_memory::GST_CAPS_FIELD_WGPU_BUFFER_USAGE);
                builder = builder.structure(new_s);
            }

            builder.build()
        } else {
            let mut builder = gst::Caps::builder_full();
            let feature = gst::CapsFeatures::new([
                deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER,
            ]);

            for s in caps.iter() {
                builder = Self::sink_allowed_usages()
                    .into_iter()
                    .map(|usage| usage.bits())
                    .fold(builder, |builder, item| {
                        let mut new_s = s.to_owned();
                        new_s.set(GST_CAPS_FIELD_WGPU_BUFFER_USAGE, item);
                        builder.structure_with_features(new_s, feature.clone())
                    });
            }

            builder.build()
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

    fn set_caps(&self, incaps: &gst::Caps, outcaps: &gst::Caps) -> Result<(), gst::LoggableError> {
        gst::info!(CAT, imp: self, "negotiated caps {:?} -> {:?}", incaps, outcaps);

        let Some(incaps_s) = incaps.structure(0) else {
            return Err(gst::loggable_error!(CAT, "missing structure in input caps"));
        };

        let sink_usages_bits: u32 = match incaps_s.get(GST_CAPS_FIELD_WGPU_BUFFER_USAGE) {
            Ok(usage) => usage,
            Err(err) => {
                return Err(gst::loggable_error!(
                    CAT,
                    "cannot get buffer usage in input caps: {}",
                    err
                ));
            }
        };
        let sink_usages = wgpu::BufferUsages::from_bits_truncate(sink_usages_bits);
        if !sink_usages.intersects(wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::MAP_READ) {
            return Err(gst::loggable_error!(
                CAT,
                "buffer usage({:?} in input caps cannot be mapped",
                sink_usages
            ));
        }

        *self.sink_usages.lock() = sink_usages;

        self.parent_set_caps(incaps, outcaps)
    }

    fn before_transform(&self, inbuf: &gst::BufferRef) {
        assert!(0 < inbuf.n_memory());

        let mem = inbuf.peek_memory(0);
        let old_passthrough = self.obj().is_passthrough();

        let Some(wgpu_mem) = mem.downcast_memory_ref::<WgpuBufferMemory>() else {
            gst::error!(CAT, imp: self, "incoming memory is not WGPU");
            self.obj().set_passthrough(false);
            self.obj().reconfigure_src();
            // The transform will generate error if this happen
            return;
        };

        let usages = wgpu_mem.buffer().usage();

        if usages.intersects(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE) {
            if !old_passthrough {
                gst::debug!(CAT, imp: self, "buffer({usages:?}) can be mapped as is, passthrough");
                self.obj().set_passthrough(true);
                self.obj().reconfigure_src();
            }
        } else {
            if old_passthrough {
                gst::debug!(CAT, imp: self, "buffer({usages:?}) cannot be mapped as is, will copy");
                self.obj().set_passthrough(false);
                self.obj().reconfigure_src();
            }
        }
    }

    fn transform(
        &self,
        inbuf: &gst::Buffer,
        outbuf: &mut gst::BufferRef,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        assert!(0 < inbuf.n_memory());
        assert!(0 < outbuf.n_memory());

        let inmem = inbuf.peek_memory(0);

        let Some(inmem) = inmem.downcast_memory_ref::<WgpuBufferMemory>() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let in_usages = inmem.buffer().usage();

        if !in_usages.contains(wgpu::BufferUsages::COPY_SRC) {
            gst::error!(CAT, imp: self, "input buffer({in_usages:?}) does not contain COPY_SRC");
            return Err(gst::FlowError::NotNegotiated);
        }

        let outmem = outbuf.peek_memory_mut(0).map_err(|x| {
            gst::error!(CAT, imp: self, "output buffer is not writable: {x}");
            gst::FlowError::Error
        })?;

        let Some(outmem) = outmem.downcast_memory_mut::<WgpuBufferMemory>() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let out_usages = outmem.buffer().usage();
        if !out_usages.contains(wgpu::BufferUsages::COPY_DST) {
            gst::error!(CAT, imp: self, "output buffer({out_usages:?}) does not contain COPY_DST");
            return Err(gst::FlowError::NotNegotiated);
        }

        let ctx = self.wgpu_context.lock().clone().unwrap();
        let copy_size = inmem.size().min(outmem.size()) as u64;

        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(inmem.buffer(), 0, outmem.buffer(), 0, copy_size);

        let token = ctx.queue().submit([encoder.finish()]);
        if let Err(err) = ctx.device().poll(wgpu::PollType::Wait {
            submission_index: Some(token),
            timeout: None,
        }) {
            gst::error!(CAT, imp: self, "failed to poll: {}", err);
            return Err(gst::FlowError::Error);
        }

        Ok(gst::FlowSuccess::Ok)
    }

    fn decide_allocation(
        &self,
        query: &mut gst::query::Allocation,
    ) -> Result<(), gst::LoggableError> {
        let sink_usages = self.sink_usages.lock();
        if sink_usages.is_empty() {
            return Err(gst::loggable_error!(
                CAT,
                "decide_allocation called before negotiation"
            ));
        }

        // TODO: What if element after us needs specific alignment?
        if sink_usages.intersects(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE) {
            gst::debug!(CAT, imp: self, "buffer({sink_usages:?}) can be mapped as is, passthrough");
            self.obj().set_passthrough(true);
            self.obj().reconfigure_src();
            return Ok(());
        }

        let mut to_remove = vec![];

        for (pos, (allocator, _params)) in query.allocation_params().iter().enumerate() {
            let Some(wgpu_allocator) = allocator.and_downcast_ref::<WgpuBufferMemoryAllocator>()
            else {
                gst::trace!(CAT, imp: self, "skipping allocator at {pos}, not an WGPU");
                to_remove.push(pos);
                continue;
            };

            match wgpu_allocator.explicit_usages() {
                Some(usages) => {
                    // If we here, we in NOT in passthrough mode, need to be able copy from input buffer
                    let required = wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST;
                    if !usages.contains(required) {
                        gst::trace!(CAT, imp: self, "skipping allocator at {pos}, usages is incorrect {} != {}", required.bits(), usages.bits());
                    }
                }
                None => {}
            }
        }

        for pos in to_remove.iter().rev() {
            query.remove_nth_allocation_param(*pos as u32);
        }

        if 0 < query.allocation_params().len() {
            return Ok(());
        }

        // Have to create own buffers with COPY_DST and MAP_WRITE
        let ctx = self.wgpu_context.lock().as_ref().cloned().unwrap();
        let allocator = WgpuBufferMemoryAllocator::new_with_explicit_usage(
            ctx,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        let params = gst::AllocationParams::default();
        query.add_allocation_param(Some(&allocator), params);

        Ok(())
    }

    fn propose_allocation(
        &self,
        _decide_query: Option<&gst::query::Allocation>,
        query: &mut gst::query::Allocation,
    ) -> Result<(), gst::LoggableError> {
        let (caps, _needs_pool) = query.get();

        let Some(caps) = caps else {
            return Err(gst::loggable_error!(CAT, "No caps in allocation query"));
        };

        let Some(caps_s) = caps.structure(0) else {
            return Err(gst::loggable_error!(CAT, "No structure in caps"));
        };

        let sink_usages_bits: u32 = match caps_s.get(GST_CAPS_FIELD_WGPU_BUFFER_USAGE) {
            Ok(usage) => usage,
            Err(err) => {
                return Err(gst::loggable_error!(
                    CAT,
                    "cannot get buffer usage in input caps: {}",
                    err
                ));
            }
        };
        let sink_usages = wgpu::BufferUsages::from_bits_truncate(sink_usages_bits);

        let ctx = self.wgpu_context.lock().as_ref().cloned().unwrap();

        let allocator = WgpuBufferMemoryAllocator::new_with_explicit_usage(ctx, sink_usages);
        let params = gst::AllocationParams::default();
        query.add_allocation_param(Some(&allocator), params);

        Ok(())
    }
}
