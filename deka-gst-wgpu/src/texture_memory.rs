//!
//! The GstMemory subclass for WgpuBuffers
//!

use std::sync::LazyLock;

use glib::translate::IntoGlibPtr;
use glib::translate::{from_glib, from_glib_full};
use gst::glib::subclass::types::ObjectSubclassIsExt;

use crate::{glib, skip_assert_initialized, WgpuContext};

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "gstwgputexturememory",
        gst::DebugColorFlags::empty(),
        Some("Gstreamer WGPU Texture memory"),
    )
});

/// Caps with this feature implies that the buffer is a WGPU Texture.
pub const GST_CAPS_FEATURE_MEMORY_WGPU_TEXTURE: &str = "memory:WgpuTexture";
/// The field in structure to determinate texture usage, this is bitmask, the element should allocate output buffers which will
/// contains all of required usages
pub const GST_CAPS_FIELD_WGPU_TEXTURE_USAGE: &str = "texture-usage";

pub trait WgpuTextureMemoryExt {
    fn texture(&self) -> &wgpu::Texture;
    fn context(&self) -> &WgpuContext;
}

gst::memory_object_wrapper!(
    WgpuTextureMemory,
    WgpuTextureMemoryRef,
    imp::WgpuTextureMemory,
    |mem: &gst::MemoryRef| { unsafe { from_glib(imp::gst_is_wgpu_memory(mem.as_mut_ptr())) } },
    gst::Memory,
    gst::MemoryRef
);

impl WgpuTextureMemoryExt for WgpuTextureMemory {
    fn texture(&self) -> &wgpu::Texture {
        &self.0.texture
    }

    fn context(&self) -> &WgpuContext {
        &self.0.context
    }
}

impl WgpuTextureMemoryExt for WgpuTextureMemoryRef {
    fn texture(&self) -> &wgpu::Texture {
        &self.0.texture
    }

    fn context(&self) -> &WgpuContext {
        &self.0.context
    }
}

glib::wrapper! {
    pub struct WgpuTextureMemoryAllocator(ObjectSubclass<imp::WgpuMemoryAllocator>) @extends gst::Allocator, gst::Object;
}

impl WgpuTextureMemoryAllocator {
    /// Creates allocator which uses specified buffer usage instead of figure out them
    pub fn new(context: WgpuContext, descriptor: wgpu::TextureDescriptor<'static>) -> Self {
        let out: Self = glib::Object::new();

        let imp = out.imp();
        // SAFETY: We set context one time, it does not mutate after creation
        // The creation itself cannot be parallel to be a problem
        unsafe {
            *imp.context.get() = Some(context);
            *imp.descriptor.get() = descriptor;
        };

        out
    }

    pub fn context(&self) -> WgpuContext {
        let imp = self.imp();
        let cell = unsafe { &*imp.context.get() };
        cell.as_ref().unwrap().clone()
    }

    pub fn descriptor(&self) -> &wgpu::TextureDescriptor<'static> {
        let imp = self.imp();
        let cell = unsafe { &*imp.descriptor.get() };
        cell
    }
}

mod imp {
    use std::cell::UnsafeCell;
    use std::mem::ManuallyDrop;

    use glib::object::Cast;
    use glib::object::ObjectType;
    use glib::subclass::object::{ObjectImpl, ObjectImplExt};
    use glib::subclass::types::ObjectSubclass;
    use glib::subclass::types::ObjectSubclassExt;
    use glib::translate::{FromGlibPtrBorrow, ToGlibPtr};
    use gst::subclass::prelude::*;

    use super::CAT;
    use crate::glib;
    use crate::WgpuContext;

    pub const GST_WGPU_ALLOCATOR_TYPE: &[u8] = b"RustWgpuTextureAllocator\0";

    #[repr(C)]
    pub struct WgpuTextureMemory {
        pub(super) parent: gst::ffi::GstMemory,
        pub(super) context: ManuallyDrop<WgpuContext>,
        pub(super) texture: ManuallyDrop<wgpu::Texture>,
    }

    impl std::fmt::Debug for WgpuTextureMemory {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WgpuTextureMemory")
                .field("parent", &self.parent)
                .field("context", &self.context)
                .field("texture", &self.texture)
                .finish_non_exhaustive()
        }
    }

    impl WgpuTextureMemory {}

    pub(super) unsafe extern "C" fn gst_is_wgpu_memory(
        memory: *mut gst::ffi::GstMemory,
    ) -> glib::ffi::gboolean {
        let mem = unsafe { &*memory };

        if mem.allocator.is_null() {
            return false.into();
        }

        let obj = gst::Allocator::from_glib_borrow(mem.allocator);
        if obj
            .downcast_ref::<super::WgpuTextureMemoryAllocator>()
            .is_none()
        {
            return false.into();
        }

        true.into()
    }

    /// Inits the allocators's function table
    unsafe extern "C" fn gst_wgpu_mem_allocator_init(allocator: *mut gst::ffi::GstAllocator) {
        debug_assert!(!allocator.is_null());

        (*allocator).mem_type = GST_WGPU_ALLOCATOR_TYPE.as_ptr() as *const core::ffi::c_char;
        (*allocator).mem_map = None;
        (*allocator).mem_unmap = None;
        (*allocator).mem_copy = None; // TODO
        (*allocator).mem_share = None; // TODO
        (*allocator).mem_is_span = None;
    }

    #[derive(Debug)]
    pub struct WgpuMemoryAllocator {
        pub(super) context: UnsafeCell<Option<WgpuContext>>,
        pub(super) descriptor: UnsafeCell<wgpu::TextureDescriptor<'static>>,
    }

    impl WgpuMemoryAllocator {
        #[inline]
        fn context(&self) -> &WgpuContext {
            let ctx = unsafe { &*self.context.get() };
            ctx.as_ref().unwrap()
        }

        #[inline]
        fn device(&self) -> &wgpu::Device {
            self.context().device()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WgpuMemoryAllocator {
        const NAME: &'static str = "WgpuTextureMemoryAllocator";
        type Type = super::WgpuTextureMemoryAllocator;
        type ParentType = gst::Allocator;

        fn with_class(_class: &Self::Class) -> Self {
            Self {
                context: Default::default(),
                descriptor: UnsafeCell::new(wgpu::TextureDescriptor {
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    label: None,
                    mip_level_count: 1,
                    sample_count: 1,
                    size: wgpu::Extent3d {
                        width: 256,
                        height: 256,
                        depth_or_array_layers: 1,
                    },
                    usage: wgpu::TextureUsages::empty(),
                    view_formats: &[],
                }),
            }
        }
    }

    impl ObjectImpl for WgpuMemoryAllocator {
        fn constructed(&self) {
            let obj = self.obj();
            let allocator_obj = obj.upcast_ref::<gst::Allocator>();
            let allocator_ptr: *mut gst::ffi::GstAllocator = allocator_obj.to_glib_none().0;

            unsafe {
                gst_wgpu_mem_allocator_init(allocator_ptr);
            }

            self.parent_constructed();
        }
    }
    impl GstObjectImpl for WgpuMemoryAllocator {}
    impl AllocatorImpl for WgpuMemoryAllocator {
        fn alloc(
            &self,
            size: usize,
            params: Option<&gst::AllocationParams>,
        ) -> Result<gst::Memory, glib::BoolError> {
            let layout = core::alloc::Layout::new::<WgpuTextureMemory>();
            // SAFETY: layout have non zero size: WgpuMemory sized fields
            let mem = unsafe { std::alloc::alloc_zeroed(layout) } as *mut WgpuTextureMemory;

            let mut align = wgpu::MAP_ALIGNMENT as usize - 1;
            let offset;
            let mut maxsize = size;
            let flags;

            let p = params.cloned().unwrap_or_default();
            flags = p.flags().bits();
            align |= p.align();
            offset = p.prefix();
            maxsize += p.prefix() + p.padding();

            let gst_allocator_ptr =
                self.obj().as_object_ref().to_glib_full() as *mut gst::ffi::GstAllocator;

            unsafe {
                gst::ffi::gst_memory_init(
                    mem as *mut gst::ffi::GstMemory,
                    flags,
                    gst_allocator_ptr,
                    core::ptr::null_mut(),
                    maxsize,
                    align,
                    offset,
                    size,
                )
            };

            let mem_flags = gst::MemoryFlags::from_bits_truncate(flags);

            if !mem_flags.contains(gst::MemoryFlags::NOT_MAPPABLE) {
                gst::warning!(CAT, imp: self, "trying to alloc tetxure without NOT_MAPPABLE set. Wgpu Textures cannot be mapped!");
            }

            let wgpu_texture = self
                .device()
                .create_texture(unsafe { &*self.descriptor.get() });

            unsafe {
                core::ptr::write(
                    &raw mut (*mem).context,
                    ManuallyDrop::new(self.context().clone()),
                );
                core::ptr::write(&raw mut (*mem).texture, ManuallyDrop::new(wgpu_texture));
            }

            gst::debug!(CAT, "allocated buffer {:p}, maxsize {}", mem, maxsize);

            let out_mem = unsafe { gst::Memory::from_glib_full(mem as *mut gst::ffi::GstMemory) };
            Ok(out_mem)
        }

        fn free(&self, memory: gst::Memory) {
            let mut wgpu_mem: super::WgpuTextureMemory =
                memory.downcast_memory().expect("non wgpu mem passed");
            let wgpu_mem_obj = unsafe { wgpu_mem.obj.as_mut() };
            unsafe {
                ManuallyDrop::drop(&mut wgpu_mem_obj.context);
            };
            unsafe {
                ManuallyDrop::drop(&mut wgpu_mem_obj.texture);
            };

            // At this point allocator might be lost, do not use it after
            unsafe {
                gst::ffi::gst_mini_object_unref(
                    wgpu_mem_obj.parent.allocator as *mut gst::ffi::GstMiniObject,
                )
            };

            let layout = core::alloc::Layout::new::<WgpuTextureMemory>();
            unsafe { std::alloc::dealloc(wgpu_mem.as_mut_ptr() as *mut u8, layout) };
            gst::debug!(CAT, "free buffer {:p}", wgpu_mem.as_mut_ptr());
            std::mem::forget(wgpu_mem); // We dealloc the memory ourselves
        }
    }

    unsafe impl Send for WgpuMemoryAllocator {}
    unsafe impl Sync for WgpuMemoryAllocator {}
}
