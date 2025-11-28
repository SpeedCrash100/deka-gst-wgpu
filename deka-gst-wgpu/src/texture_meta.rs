//!
//! Meta information attached to buffers that contains an texture which were uploaded to GPU
//!

use gst::{MetaAPI, MetaAPIExt};

use crate::{glib, WgpuContext};

/// GstCapsFeature that tells that the buffer has a WGPU texture attached with uploaded content of WGPU buffer to this structure
pub const GST_CAPS_FEATURE_META_WGPU_TEXTURE: &str = "meta:WgpuTextureMetaAPI";

/// Field used to sync texture usages between elements
pub const GST_CAPS_FIELD_WGPU_TEXTURE_USAGE: &str = "texture-usages";

#[repr(transparent)]
pub struct WgpuTextureMeta(imp::WgpuTextureMeta);

impl WgpuTextureMeta {
    pub fn add(
        dst: &mut gst::BufferRef,
        context: WgpuContext,
        texture: wgpu::Texture,
    ) -> gst::MetaRefMut<'_, Self, gst::meta::Standalone> {
        let mut params = imp::WgpuTextureMetaParams { context, texture };
        let meta = unsafe {
            gst::ffi::gst_buffer_add_meta(
                dst.as_mut_ptr(),
                imp::wgpu_texture_meta_get_info(),
                &raw mut params as glib::ffi::gpointer,
            )
        };

        unsafe { Self::from_mut_ptr(dst, meta as *mut imp::WgpuTextureMeta) }
    }

    pub fn context(&self) -> &WgpuContext {
        &self.0.context
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.0.texture
    }
}

unsafe impl Send for WgpuTextureMeta {}
unsafe impl Sync for WgpuTextureMeta {}

unsafe impl MetaAPI for WgpuTextureMeta {
    type GstType = imp::WgpuTextureMeta;

    fn meta_api() -> gst::glib::Type {
        imp::wgpu_texture_meta_api_get_type()
    }
}

impl core::fmt::Debug for WgpuTextureMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuTextureMeta")
            .field("context", self.context())
            .field("texture", self.texture())
            .finish_non_exhaustive()
    }
}

mod imp {
    use std::{mem::ManuallyDrop, sync::LazyLock};

    use gst::glib::translate::{from_glib, IntoGlib};

    use crate::{glib, WgpuContext};

    pub(super) struct WgpuTextureMetaParams {
        pub context: WgpuContext,
        pub texture: wgpu::Texture,
    }

    #[repr(C)]
    pub struct WgpuTextureMeta {
        parent: gst::ffi::GstMeta,
        pub(super) context: ManuallyDrop<WgpuContext>,
        pub(super) texture: ManuallyDrop<wgpu::Texture>,
    }

    #[no_mangle]
    pub(super) extern "C" fn wgpu_texture_meta_api_get_type() -> glib::Type {
        static TYPE: LazyLock<glib::Type> = LazyLock::new(|| unsafe {
            let t = from_glib(gst::ffi::gst_meta_api_type_register(
                c"GstWgpuTextureMetaAPI".as_ptr() as *const _,
                [core::ptr::null::<std::os::raw::c_char>()].as_ptr() as *mut *const _,
            ));

            assert_ne!(t, glib::Type::INVALID);

            t
        });

        *TYPE
    }

    unsafe extern "C" fn wgpu_texture_meta_init(
        meta: *mut gst::ffi::GstMeta,
        params: glib::ffi::gpointer,
        _dst: *mut gst::ffi::GstBuffer,
    ) -> glib::ffi::gboolean {
        let meta = &mut *(meta as *mut WgpuTextureMeta);
        let params = core::ptr::read(params as *const WgpuTextureMetaParams);

        let WgpuTextureMetaParams { context, texture } = params;

        core::ptr::write(&mut meta.context, ManuallyDrop::new(context));
        core::ptr::write(&mut meta.texture, ManuallyDrop::new(texture));

        true.into_glib()
    }

    unsafe extern "C" fn wgpu_texture_meta_free(
        meta: *mut gst::ffi::GstMeta,
        _buffer_attached_to: *mut gst::ffi::GstBuffer,
    ) {
        let meta = &mut *(meta as *mut WgpuTextureMeta);
        ManuallyDrop::drop(&mut meta.context);
        ManuallyDrop::drop(&mut meta.texture);
    }

    unsafe extern "C" fn wgpu_texture_meta_transform(
        dst: *mut gst::ffi::GstBuffer,
        meta: *mut gst::ffi::GstMeta,
        _src: *mut gst::ffi::GstBuffer,
        _type_: glib::ffi::GQuark,
        _data: glib::ffi::gpointer,
    ) -> glib::ffi::gboolean {
        let dst = gst::BufferRef::from_mut_ptr(dst);
        if dst.meta::<super::WgpuTextureMeta>().is_some() {
            // Already exists
            return true.into_glib();
        }

        let meta = &*(meta as *const WgpuTextureMeta);

        let context: &WgpuContext = &meta.context;
        let texture: &wgpu::Texture = &meta.texture;

        super::WgpuTextureMeta::add(dst, context.clone(), texture.clone());

        true.into_glib()
    }

    pub(super) fn wgpu_texture_meta_get_info() -> *const gst::ffi::GstMetaInfo {
        struct MetaInfo(core::ptr::NonNull<gst::ffi::GstMetaInfo>);
        unsafe impl Send for MetaInfo {}
        unsafe impl Sync for MetaInfo {}

        static META_INFO: LazyLock<MetaInfo> = LazyLock::new(|| unsafe {
            MetaInfo(
                core::ptr::NonNull::new(gst::ffi::gst_meta_register(
                    wgpu_texture_meta_api_get_type().into_glib(),
                    c"WgpuTextureMeta".as_ptr() as *const _,
                    core::mem::size_of::<WgpuTextureMeta>(),
                    Some(wgpu_texture_meta_init),
                    Some(wgpu_texture_meta_free),
                    Some(wgpu_texture_meta_transform),
                ) as *mut gst::ffi::GstMetaInfo)
                .expect("Failed to register meta API"),
            )
        });

        META_INFO.0.as_ptr()
    }
}
