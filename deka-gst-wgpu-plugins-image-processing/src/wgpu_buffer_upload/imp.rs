use std::sync::LazyLock;
use std::time::Duration;

use crate::glib;

use deka_gst_wgpu::buffer_memory::WgpuBufferMemory;
use deka_gst_wgpu::{prelude::*, WgpuBufferMemoryAllocator};
use glib::object::Cast;
use glib::subclass::{object::ObjectImpl, types::ObjectSubclass};
use gst::prelude::ElementExt;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::{BaseTransformImpl, BaseTransformImplExt};
use gst_base::subclass::BaseTransformMode;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
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
}

#[glib::object_subclass]
impl ObjectSubclass for WgpuBufferUpload {
    const NAME: &'static str = "GstWgpuBufferUpload";
    type Type = super::WgpuBufferUpload;
    type ParentType = gst_base::BaseTransform;

    fn with_class(_klass: &Self::Class) -> Self {
        Self {
            wgpu_context: Mutex::new(None),
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
            let sink_caps = gst::Caps::new_any();
            let src_caps = gst::Caps::new_any();

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

    fn transform(
        &self,
        inbuf: &gst::Buffer,
        outbuf: &mut gst::BufferRef,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        assert_eq!(inbuf.n_memory(), 1);
        assert_eq!(outbuf.n_memory(), 1);

        let inmem = inbuf
            .memory(0)
            .unwrap()
            .downcast_memory::<WgpuBufferMemory>()
            .unwrap();

        let outmem = outbuf
            .memory(0)
            .unwrap()
            .downcast_memory::<WgpuBufferMemory>()
            .unwrap();

        let Some(wgpu_context) = &*self.wgpu_context.lock() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let mut encoder = wgpu_context
            .device()
            .create_command_encoder(&Default::default());

        encoder.copy_buffer_to_buffer(inmem.buffer(), 0, outmem.buffer(), 0, inmem.buffer().size());
        wgpu_context.queue().submit([encoder.finish()]);

        Ok(gst::FlowSuccess::Ok)
    }

    fn unit_size(&self, caps: &gst::Caps) -> Option<usize> {
        let video_caps = gst_video::VideoInfo::from_caps(&caps).ok()?;

        Some(video_caps.size())
    }

    fn propose_allocation(
        &self,
        _decide_query: Option<&gst::query::Allocation>,
        query: &mut gst::query::Allocation,
    ) -> Result<(), gst::LoggableError> {
        let allocator =
            WgpuBufferMemoryAllocator::new(self.wgpu_context.lock().as_ref().cloned().unwrap());
        // Default params for MAP_WRITE buffers
        let params = gst::AllocationParams::default();
        query.add_allocation_param(Some(&allocator), params);

        Ok(())
    }
}
