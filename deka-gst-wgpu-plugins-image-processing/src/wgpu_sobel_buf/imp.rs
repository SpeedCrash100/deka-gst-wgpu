use std::sync::LazyLock;

use deka_gst_wgpu::{
    buffer_memory::GST_CAPS_FIELD_WGPU_BUFFER_USAGE, caps::make_wgpu_buffer_usages_for_caps,
    prelude::WgpuBufferMemoryExt, WgpuBufferMemory, WgpuBufferMemoryAllocator, WgpuContext,
    GST_CONTEXT_WGPU_TYPE,
};
use gst::{
    glib::{
        object::Cast,
        subclass::{object::ObjectImpl, types::ObjectSubclass},
    },
    prelude::ElementExt,
    subclass::prelude::*,
};
use gst_base::subclass::{prelude::*, BaseTransformMode};
use gst_video::{prelude::*, subclass::prelude::*};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};

use crate::glib;

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "dekawgpusobelbuf",
        gst::DebugColorFlags::empty(),
        Some("Deka's WebGPU sobel filter which uses custom memory"),
    )
});

#[derive(Debug)]
struct WebGPUState {
    input_texture: wgpu::Texture,
    output_texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
}

#[derive(Debug)]
pub struct WgpuSobelBuf {
    wgpu_context: Mutex<Option<WgpuContext>>,
    pipeline: Mutex<Option<WebGPUState>>,
    usages: Mutex<(wgpu::BufferUsages, wgpu::BufferUsages)>,
}

impl WgpuSobelBuf {
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
            wgpu::BufferUsages::COPY_SRC,
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE,
        ]
    }

    fn src_allowed_usages() -> impl IntoIterator<Item = wgpu::BufferUsages> {
        [
            wgpu::BufferUsages::COPY_DST,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        ]
    }

    fn lock_src_usages(&self) -> MappedMutexGuard<'_, wgpu::BufferUsages> {
        let usages = self.usages.lock();
        MutexGuard::map(usages, |(_sink, src)| src)
    }
    fn lock_sink_usages(&self) -> MappedMutexGuard<'_, wgpu::BufferUsages> {
        let usages = self.usages.lock();
        MutexGuard::map(usages, |(sink, _src)| sink)
    }
}

#[glib::object_subclass]
impl ObjectSubclass for WgpuSobelBuf {
    const NAME: &'static str = "GstWgpuSobelBuf";
    type Type = super::WgpuSobelBuf;
    type ParentType = gst_video::VideoFilter;

    fn with_class(_klass: &Self::Class) -> Self {
        Self {
            wgpu_context: Mutex::new(None),
            pipeline: Mutex::new(None),
            usages: Mutex::new((wgpu::BufferUsages::empty(), wgpu::BufferUsages::empty())),
        }
    }
}

impl ObjectImpl for WgpuSobelBuf {}
impl GstObjectImpl for WgpuSobelBuf {}
impl ElementImpl for WgpuSobelBuf {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "Deka's WebGPU sobel filter which uses custom memory",
                "Filter/Effect/Video",
                "Applies a sobel filter to the input video frame",
                "Deka <speedcrash100@ya.ru>",
            )
        });
        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
            let base_caps = gst_video::VideoCapsBuilder::new()
                .format(gst_video::VideoFormat::Rgbx)
                .features([deka_gst_wgpu::buffer_memory::GST_CAPS_FEATURE_MEMORY_WGPU_BUFFER])
                .build();

            let src_caps =
                make_wgpu_buffer_usages_for_caps(&base_caps, WgpuSobelBuf::src_allowed_usages);
            let sink_caps =
                make_wgpu_buffer_usages_for_caps(&base_caps, WgpuSobelBuf::sink_allowed_usages);

            vec![
                gst::PadTemplate::new(
                    "src",
                    gst::PadDirection::Src,
                    gst::PadPresence::Always,
                    &src_caps,
                )
                .unwrap(),
                gst::PadTemplate::new(
                    "sink",
                    gst::PadDirection::Sink,
                    gst::PadPresence::Always,
                    &sink_caps,
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

impl BaseTransformImpl for WgpuSobelBuf {
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
        let other_caps = if direction == gst::PadDirection::Sink {
            make_wgpu_buffer_usages_for_caps(caps, Self::src_allowed_usages)
        } else {
            make_wgpu_buffer_usages_for_caps(caps, Self::sink_allowed_usages)
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

        if !sink_usages.contains(wgpu::BufferUsages::COPY_SRC) {
            return Err(gst::loggable_error!(
                CAT,
                "input caps({:?}) cannot be used as copy src",
                sink_usages
            ));
        }

        if !src_usages.contains(wgpu::BufferUsages::COPY_DST) {
            return Err(gst::loggable_error!(
                CAT,
                "output caps({:?}) cannot be used as copy dst",
                src_usages
            ));
        }

        {
            let mut usages_lock = self.usages.lock();
            *usages_lock = (sink_usages, src_usages);
        }

        self.parent_set_caps(incaps, outcaps)
    }

    fn transform(
        &self,
        inbuf: &gst::Buffer,
        outbuf: &mut gst::BufferRef,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        let Some(mem) = inbuf
            .peek_memory(0)
            .downcast_memory_ref::<WgpuBufferMemory>()
        else {
            gst::error!(CAT, imp: self, "unsupported memory");
            return Err(gst::FlowError::NotNegotiated);
        };

        let mem_usage = mem.buffer().usage();
        if !mem_usage.contains(wgpu::BufferUsages::COPY_SRC) {
            gst::error!(CAT, imp: self, "memory({:?}) missing COPY_SRC", mem_usage);
            return Err(gst::FlowError::NotNegotiated);
        }

        let obj = self.obj();
        let self_as_filter = obj.upcast_ref::<gst_video::VideoFilter>();
        let Some(in_info) = self_as_filter.input_video_info() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let Some(out_info) = self_as_filter.output_video_info() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let Some(pipeline) = &*self.pipeline.lock() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let Some(wgpu_context) = &*self.wgpu_context.lock() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let inbuffer = mem.buffer();

        let outmem = match outbuf.peek_memory_mut(0) {
            Ok(m) => m,
            Err(err) => {
                gst::error!(CAT, imp: self, "can't get mutable out memory: {err}");
                return Err(gst::FlowError::NotNegotiated);
            }
        };

        let Some(outmem) = outmem.downcast_memory_mut::<WgpuBufferMemory>() else {
            gst::error!(CAT, imp: self, "unsupported out memory");
            return Err(gst::FlowError::NotNegotiated);
        };

        let outbuffer = outmem.buffer();

        let mut encoder = wgpu_context
            .device()
            .create_command_encoder(&Default::default());

        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfoBase {
                buffer: &inbuffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * in_info.width()),
                    rows_per_image: None,
                },
            },
            pipeline.input_texture.as_image_copy(),
            wgpu::Extent3d {
                width: in_info.width(),
                height: in_info.height(),
                depth_or_array_layers: 1,
            },
        );

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                ..Default::default()
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &pipeline.bind_group, &[]);

            let workgroup_x = in_info.width().div_ceil(8);
            let workgroup_y = in_info.height().div_ceil(8);
            pass.dispatch_workgroups(workgroup_x, workgroup_y, 1);
        }

        encoder.copy_texture_to_buffer(
            pipeline.output_texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: outbuffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * out_info.width()),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: out_info.width(),
                height: out_info.height(),
                depth_or_array_layers: 1,
            },
        );

        let command_buffer = encoder.finish();

        wgpu_context.queue().submit([command_buffer]);

        Ok(gst::FlowSuccess::Ok)
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

impl VideoFilterImpl for WgpuSobelBuf {
    fn set_info(
        &self,
        _incaps: &gst::Caps,
        in_info: &gst_video::VideoInfo,
        _outcaps: &gst::Caps,
        out_info: &gst_video::VideoInfo,
    ) -> Result<(), gst::LoggableError> {
        let Some(wgpu_context) = &*self.wgpu_context.lock() else {
            return Err(gst::loggable_error!(CAT, "Could not find a WGPU context"));
        };
        let device = wgpu_context.device();

        let input_texture_descriptor = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: in_info.width(),
                height: in_info.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let input_texture = device.create_texture(&input_texture_descriptor);

        let output_texture_descriptor = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: out_info.width(),
                height: out_info.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        };
        let output_texture = device.create_texture(&output_texture_descriptor);

        let module = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });

        let input_texture_view = input_texture.create_view(&wgpu::TextureViewDescriptor {
            ..Default::default()
        });

        let output_texture_view = output_texture.create_view(&wgpu::TextureViewDescriptor {
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output_texture_view),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("sobel compute"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("computeSobel"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        {
            let mut pipeline = self.pipeline.lock();
            *pipeline = Some(WebGPUState {
                input_texture,
                output_texture,
                bind_group,
                pipeline: compute_pipeline,
            })
        }

        Ok(())
    }
}
