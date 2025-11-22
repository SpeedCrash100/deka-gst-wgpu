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
        "dekawgpusobelmem",
        gst::DebugColorFlags::empty(),
        Some("Deka's WebGPU sobel filter which uses custom memory"),
    )
});

#[derive(Debug)]
struct WebGPUState {
    input_buffer: wgpu::Buffer,
    input_texture: wgpu::Texture,
    output_texture: wgpu::Texture,
    output_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
}

#[derive(Debug)]
pub struct WgpuSobelMem {
    wgpu_context: Mutex<Option<WgpuContext>>,
    pipeline: Mutex<Option<WebGPUState>>,
}

impl WgpuSobelMem {
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

    fn transform_with_gpu(
        &self,
        inbuffer: &wgpu::Buffer,
        outframe: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>,
        map_input: bool,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        let Some(pipeline) = &*self.pipeline.lock() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let Some(wgpu_context) = &*self.wgpu_context.lock() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let obj = self.obj();
        let self_as_filter = obj.upcast_ref::<gst_video::VideoFilter>();
        let Some(in_info) = self_as_filter.input_video_info() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        let Some(out_info) = self_as_filter.output_video_info() else {
            return Err(gst::FlowError::NotNegotiated);
        };

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
            wgpu::TexelCopyBufferInfoBase {
                buffer: &pipeline.output_buffer,
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

        let index = wgpu_context.queue().submit([command_buffer]);

        let output_slice = pipeline.output_buffer.slice(..);
        output_slice.map_async(wgpu::MapMode::Read, |_| {}); // We depend on poll, so we don't need an callback
        if map_input {
            inbuffer.map_async(wgpu::MapMode::Write, .., |_| {});
        }; // We also map the input buffer for next iteration

        if let Err(err) = wgpu_context.device().poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: Some(Duration::from_millis(500)),
        }) {
            gst::error!(CAT, imp: self, "Error submitting command buffer: {}", err);
            return Err(gst::FlowError::Error);
        }

        // Our submission ready, all buffers should be ready
        {
            let output_mapped = output_slice.get_mapped_range();
            outframe
                .plane_data_mut(0)
                .unwrap()
                .copy_from_slice(&output_mapped);
        }

        pipeline.output_buffer.unmap();

        Ok(gst::FlowSuccess::Ok)
    }
}

#[glib::object_subclass]
impl ObjectSubclass for WgpuSobelMem {
    const NAME: &'static str = "GstWgpuSobelMem";
    type Type = super::WgpuSobelMem;
    type ParentType = gst_video::VideoFilter;

    fn with_class(_klass: &Self::Class) -> Self {
        Self {
            wgpu_context: Mutex::new(None),
            pipeline: Mutex::new(None),
        }
    }
}

impl ObjectImpl for WgpuSobelMem {}
impl GstObjectImpl for WgpuSobelMem {}
impl ElementImpl for WgpuSobelMem {
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
            let caps = gst_video::VideoCapsBuilder::new()
                .format(gst_video::VideoFormat::Rgbx)
                .build();
            vec![
                gst::PadTemplate::new(
                    "src",
                    gst::PadDirection::Src,
                    gst::PadPresence::Always,
                    &caps,
                )
                .unwrap(),
                gst::PadTemplate::new(
                    "sink",
                    gst::PadDirection::Sink,
                    gst::PadPresence::Always,
                    &caps,
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

impl BaseTransformImpl for WgpuSobelMem {
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
        let mem = inbuf
            .iter_memories()
            .find_map(|x| x.downcast_memory_ref::<WgpuBufferMemory>());

        let obj = self.obj();
        let self_as_filter = obj.upcast_ref::<gst_video::VideoFilter>();
        let Some(in_info) = self_as_filter.input_video_info() else {
            return Err(gst::FlowError::NotNegotiated);
        };

        if let Some(gpu_mem) = mem {
            let Ok(mut outframe) =
                gst_video::VideoFrameRef::from_buffer_ref_writable(outbuf, &in_info)
            else {
                return Err(gst::FlowError::NotNegotiated);
            };
            self.transform_with_gpu(gpu_mem.buffer(), &mut outframe, false)
        } else {
            // Fallback to copy
            gst::warning!(CAT, imp: self, "using ineffective copy");
            self.parent_transform(inbuf, outbuf)
        }
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

impl VideoFilterImpl for WgpuSobelMem {
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

        let channels = 4; // RGBx
        let in_frame_size = in_info.width() as u64 * in_info.height() as u64 * channels;
        let out_frame_size = out_info.width() as u64 * out_info.height() as u64 * channels;

        // This buffer will be used to copy the input frame into.
        let input_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("input frame buffer"),
            mapped_at_creation: true,
            size: in_frame_size,
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
        });

        let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("output frame buffer"),
            mapped_at_creation: false,
            size: out_frame_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        });

        let texture_descriptor = wgpu::TextureDescriptor {
            label: Some("input texture"),
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

        let input_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("input texture"),
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            ..texture_descriptor
        });

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output texture"),
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING,
            ..texture_descriptor
        });

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
                input_buffer,
                input_texture,
                output_texture,
                output_buffer,
                bind_group,
                pipeline: compute_pipeline,
            })
        }

        Ok(())
    }

    fn transform_frame(
        &self,
        inframe: &gst_video::VideoFrameRef<&gst::BufferRef>,
        outframe: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        let input_buffer = {
            let Some(pipeline) = &*self.pipeline.lock() else {
                return Err(gst::FlowError::NotNegotiated);
            };
            pipeline.input_buffer.clone()
        };

        let input_slice = input_buffer.slice(..);
        {
            let mut input_mapped = input_slice.get_mapped_range_mut();
            input_mapped.copy_from_slice(inframe.plane_data(0).unwrap());
        }

        input_buffer.unmap();

        self.transform_with_gpu(&input_buffer, outframe, true)
    }
}
