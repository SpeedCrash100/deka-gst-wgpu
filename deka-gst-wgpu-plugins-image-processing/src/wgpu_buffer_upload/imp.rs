use std::sync::LazyLock;

use crate::glib;

use deka_gst_wgpu::buffer_memory::{WgpuBufferMemory, GST_CAPS_FIELD_WGPU_BUFFER_USAGE};
use deka_gst_wgpu::{prelude::*, WgpuBufferMemoryAllocator};
use glib::object::Cast;
use glib::subclass::{object::ObjectImpl, types::ObjectSubclass};
use gst::prelude::ElementExt;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::*;
use gst_base::subclass::BaseTransformMode;
use gst_video::prelude::*;
use parking_lot::Mutex;

use deka_gst_wgpu::{WgpuContext, GST_CONTEXT_WGPU_TYPE};

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "dekawgpubufferupload",
        gst::DebugColorFlags::empty(),
        Some("Deka's WebGPU upload to buffer plugin"),
    )
});

#[derive(Debug)]
pub struct WgpuBufferUpload {
    wgpu_context: Mutex<Option<WgpuContext>>,
    src_usages: Mutex<wgpu::BufferUsages>,
}

impl WgpuBufferUpload {
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

    fn src_allowed_usages() -> impl IntoIterator<Item = wgpu::BufferUsages> {
        [
            wgpu::BufferUsages::MAP_WRITE,
            wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
        ]
    }
}

#[glib::object_subclass]
impl ObjectSubclass for WgpuBufferUpload {
    const NAME: &'static str = "GstWgpuBufferUpload";
    type Type = super::WgpuBufferUpload;
    type ParentType = gst_base::BaseTransform;

    fn with_class(_klass: &Self::Class) -> Self {
        Self {
            wgpu_context: Mutex::new(None),
            src_usages: Mutex::new(wgpu::BufferUsages::empty()),
        }
    }
}

impl ObjectImpl for WgpuBufferUpload {}
impl GstObjectImpl for WgpuBufferUpload {}
impl ElementImpl for WgpuBufferUpload {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "Deka's WebGPU Buffer Upload plugin",
                "Filter/Effect/Video",
                "Uploads buffer to GPU using WebGPU",
                "Deka <speedcrash100@ya.ru>",
            )
        });
        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
            let sink_caps = gst::Caps::builder_full()
                .structure(gst::Structure::new_empty("audio/x-raw"))
                .structure(gst::Structure::new_empty("video/x-raw"))
                .build();

            let mem_feature = gst::CapsFeatures::new([
                deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER,
            ]);

            let src_caps_builder = WgpuBufferUpload::src_allowed_usages()
                .into_iter()
                .map(|usage| usage.bits())
                .fold(gst::Caps::builder_full(), |builder, bits| {
                    builder
                        .structure_with_features(
                            gst::Structure::builder("audio/x-raw")
                                .field(GST_CAPS_FIELD_WGPU_BUFFER_USAGE, bits)
                                .build(),
                            mem_feature.clone(),
                        )
                        .structure_with_features(
                            gst::Structure::builder("video/x-raw")
                                .field(GST_CAPS_FIELD_WGPU_BUFFER_USAGE, bits)
                                .build(),
                            mem_feature.clone(),
                        )
                });

            let src_caps = src_caps_builder.build();

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

impl BaseTransformImpl for WgpuBufferUpload {
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
        let other_caps = if direction == gst::PadDirection::Src {
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
                builder = Self::src_allowed_usages()
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

        let Some(outcaps_s) = outcaps.structure(0) else {
            return Err(gst::loggable_error!(
                CAT,
                "missing structure in output caps"
            ));
        };

        let src_usages_bits: u32 = match outcaps_s.get(GST_CAPS_FIELD_WGPU_BUFFER_USAGE) {
            Ok(usage) => usage,
            Err(err) => {
                return Err(gst::loggable_error!(
                    CAT,
                    "cannot get buffer usage in input caps: {}",
                    err
                ));
            }
        };
        let src_usages = wgpu::BufferUsages::from_bits_truncate(src_usages_bits);
        if !src_usages.intersects(wgpu::BufferUsages::MAP_WRITE) {
            return Err(gst::loggable_error!(
                CAT,
                "buffer usage({:?} in output caps cannot be mapped for write",
                src_usages
            ));
        }

        *self.src_usages.lock() = src_usages;

        self.parent_set_caps(incaps, outcaps)
    }

    fn before_transform(&self, inbuf: &gst::BufferRef) {
        assert!(0 < inbuf.n_memory());

        let mem = inbuf.peek_memory(0);
        let old_passthrough = self.obj().is_passthrough();

        let Some(wgpu_mem) = mem.downcast_memory_ref::<WgpuBufferMemory>() else {
            if old_passthrough == true {
                gst::warning!(CAT, imp: self, "the previous element does not use our allocator, have to copy");
                self.obj().set_passthrough(false);
                self.obj().reconfigure_src();
            }
            return;
        };

        if self.locked_context().as_ptr() != wgpu_mem.context().as_ptr() {
            // TODO: handle it somehow
            panic!("context not in sync");
        }

        // If we are here, the memory is WgpuMemory we can pass as is
        if old_passthrough == false {
            gst::debug!(CAT, imp: self, "the previous element uses our allocator, passthrough mode");
            self.obj().set_passthrough(true);
            self.obj().reconfigure_src();
        }
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

        let mut outmem = outbuf
            .memory(0)
            .unwrap()
            .downcast_memory::<WgpuBufferMemory>()
            .unwrap();

        outmem.fill_from_gst(inmem).map_err(|e| {
            gst::error!(CAT, imp: self, "Error copying memory: {e}");
            gst::FlowError::Error
        })?;

        Ok(gst::FlowSuccess::Ok)
    }

    fn unit_size(&self, caps: &gst::Caps) -> Option<usize> {
        let video_caps = gst_video::VideoInfo::from_caps(&caps).ok()?;

        Some(video_caps.size())
    }

    fn decide_allocation(
        &self,
        query: &mut gst::query::Allocation,
    ) -> Result<(), gst::LoggableError> {
        // if we are here, we need to copy from system memory, creating buffer
        // with MAP_WRITE required for output buffer
        //

        let src_usages = self.src_usages.lock();
        if src_usages.is_empty() {
            return Err(gst::loggable_error!(
                CAT,
                "decide_allocation called before negotiation"
            ));
        }

        let mut to_remove = vec![];

        for (pos, (allocator, params)) in query.allocation_params().iter().enumerate() {
            let Some(wgpu_allocator) = allocator.and_downcast_ref::<WgpuBufferMemoryAllocator>()
            else {
                gst::trace!(CAT, imp: self, "skipping allocator at {pos}, not an WGPU");
                to_remove.push(pos);
                continue;
            };

            match wgpu_allocator.explicit_usages() {
                Some(usages) => {
                    let required = wgpu::BufferUsages::MAP_WRITE;
                    if !usages.contains(required) {
                        gst::trace!(CAT, imp: self, "skipping allocator at {pos}, usages is incorrect {} != {}", required.bits(), usages.bits());
                    }
                }
                None => {
                    // We are ok, if allocation params is not READONLY
                    if params.flags().contains(gst::MemoryFlags::READONLY) {
                        gst::trace!(CAT, imp: self, "skipping allocator at {pos}, READONLY");
                        to_remove.push(pos);
                    }
                }
            }
        }

        for pos in to_remove.iter().rev() {
            query.remove_nth_allocation_param(*pos as u32);
        }

        if 0 < query.allocation_params().len() {
            return Ok(());
        }

        let ctx = self.wgpu_context.lock().as_ref().cloned().unwrap();
        let allocator = WgpuBufferMemoryAllocator::new_with_explicit_usage(ctx, *src_usages);
        let params = gst::AllocationParams::default();
        query.add_allocation_param(Some(&allocator), params);

        Ok(())
    }

    fn propose_allocation(
        &self,
        _decide_query: Option<&gst::query::Allocation>,
        query: &mut gst::query::Allocation,
    ) -> Result<(), gst::LoggableError> {
        let src_usages = self.src_usages.lock();
        if src_usages.is_empty() {
            return Err(gst::loggable_error!(
                CAT,
                "propose_allocation called before negotiation"
            ));
        }

        let ctx = self.wgpu_context.lock().as_ref().cloned().unwrap();

        let allocator = WgpuBufferMemoryAllocator::new_with_explicit_usage(ctx, *src_usages);
        let params = gst::AllocationParams::default();
        query.add_allocation_param(Some(&allocator), params);

        Ok(())
    }
}
