use std::sync::LazyLock;

use crate::glib;

use deka_gst_wgpu::buffer_memory::WgpuBufferMemory;
use deka_gst_wgpu::{prelude::*, WgpuBufferMemoryAllocator};
use glib::object::Cast;
use glib::subclass::{object::ObjectImpl, types::ObjectSubclass};
use gst::prelude::ElementExt;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::BaseTransformImpl;
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
}

#[glib::object_subclass]
impl ObjectSubclass for WgpuBufferDownload {
    const NAME: &'static str = "GstWgpuBufferDownload";
    type Type = super::WgpuBufferDownload;
    type ParentType = gst_base::BaseTransform;

    fn with_class(_klass: &Self::Class) -> Self {
        Self {
            wgpu_context: Mutex::new(None),
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
            // We are OK to accept anything we can copy from to output buffer
            let sink_buffer_usages = wgpu::BufferUsages::COPY_SRC;

            let sink_caps = gst::Caps::builder_full()
                .structure_with_features(
                    gst::Structure::builder("audio/x-raw")
                        .field(
                            deka_gst_wgpu::buffer_memory::GST_CAPS_FIELD_WGPU_BUFFER_USAGE,
                            sink_buffer_usages.bits(),
                        )
                        .build(),
                    gst::CapsFeatures::new([
                        deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER,
                    ]),
                )
                .structure_with_features(
                    gst::Structure::builder("video/x-raw")
                        .field(
                            deka_gst_wgpu::buffer_memory::GST_CAPS_FIELD_WGPU_BUFFER_USAGE,
                            sink_buffer_usages.bits(),
                        )
                        .build(),
                    gst::CapsFeatures::new([
                        deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER,
                    ]),
                )
                .build();
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

            for s in caps.iter() {
                let mut new_s = s.to_owned();
                new_s.set(
                    deka_gst_wgpu::buffer_memory::GST_CAPS_FIELD_WGPU_BUFFER_USAGE,
                    wgpu::BufferUsages::COPY_SRC.bits(),
                );
                builder = builder.structure_with_features(
                    new_s,
                    gst::CapsFeatures::new([
                        deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER,
                    ]),
                );
            }

            builder.build()
        };

        gst::debug!(
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
        // We want have allocation where MAP_WRITE anc COPY_SRC available

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
                    let required = wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC;
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

        if 0 == query.allocation_params().len() {
            // Have to create own, it is same as we propose
            self.propose_allocation(None, query)?;
        }

        Ok(())
    }

    fn propose_allocation(
        &self,
        _decide_query: Option<&gst::query::Allocation>,
        query: &mut gst::query::Allocation,
    ) -> Result<(), gst::LoggableError> {
        let usages = wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST; // TODO get from caps
        let ctx = self.wgpu_context.lock().as_ref().cloned().unwrap();

        let allocator = WgpuBufferMemoryAllocator::new_with_explicit_usage(ctx, usages);
        let params = gst::AllocationParams::new(gst::MemoryFlags::READONLY, 0, 0, 0);

        query.add_allocation_param(Some(&allocator), params);

        Ok(())
    }
}
