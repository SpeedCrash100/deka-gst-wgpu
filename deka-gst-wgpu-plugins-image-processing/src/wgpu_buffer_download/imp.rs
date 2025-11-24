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
            let src_caps = gst::Caps::new_any();
            let sink_caps = gst::Caps::new_any();

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
    const MODE: BaseTransformMode = BaseTransformMode::AlwaysInPlace;
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

    fn transform_ip(&self, _buf: &mut gst::BufferRef) -> Result<gst::FlowSuccess, gst::FlowError> {
        Ok(gst::FlowSuccess::Ok)
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
