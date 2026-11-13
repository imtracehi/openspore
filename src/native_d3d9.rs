//! Windows HAL replay of native packets. No shader translation or PBR fallback.
use crate::native_packet::*;
use serde::Serialize;
use windows::{
    core::w,
    Win32::{
        Foundation::*, Graphics::Direct3D9::*, System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
};

type Result<T> = std::result::Result<T, String>;
fn checked<T>(r: windows::core::Result<T>) -> Result<T> {
    r.map_err(|e| format!("D3D9/Win32: {e}"))
}
fn required<T>(v: Option<T>) -> Result<T> {
    v.ok_or_else(|| "D3D9 returned no interface".into())
}
#[derive(Clone, Debug, Serialize)]
pub struct Adapter {
    pub ordinal: u32,
    pub description: String,
    pub vendor: u32,
    pub device: u32,
    pub driver_version: i64,
    pub width: u32,
    pub height: u32,
    pub vertex_shader_version: u32,
    pub pixel_shader_version: u32,
}
fn ctext(bytes: &[i8]) -> String {
    let bytes: Vec<u8> = bytes
        .iter()
        .take_while(|v| **v != 0)
        .map(|v| *v as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
pub fn adapters() -> Result<Vec<Adapter>> {
    unsafe {
        let api = required(Direct3DCreate9(D3D_SDK_VERSION))?;
        let mut result = vec![];
        for ordinal in 0..api.GetAdapterCount() {
            let mut id = D3DADAPTER_IDENTIFIER9::default();
            checked(api.GetAdapterIdentifier(ordinal, 0, &mut id))?;
            let mut display = D3DDISPLAYMODE::default();
            checked(api.GetAdapterDisplayMode(ordinal, &mut display))?;
            let mut caps = D3DCAPS9::default();
            // Virtual adapters without HAL support are omitted rather than selected silently.
            if api
                .GetDeviceCaps(ordinal, D3DDEVTYPE_HAL, &mut caps)
                .is_err()
            {
                continue;
            }
            result.push(Adapter {
                ordinal,
                description: ctext(&id.Description),
                vendor: id.VendorId,
                device: id.DeviceId,
                driver_version: id.DriverVersion,
                width: display.Width,
                height: display.Height,
                vertex_shader_version: caps.VertexShaderVersion,
                pixel_shader_version: caps.PixelShaderVersion,
            });
        }
        Ok(result)
    }
}
struct HiddenWindow(HWND);
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, message, wparam, lparam)
}
impl HiddenWindow {
    fn new() -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = checked(GetModuleHandleW(None))?.into();
            static REGISTERED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if !*REGISTERED.get_or_init(|| {
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: w!("OpenSporeNativeReplay"),
                    ..Default::default()
                }) != 0
            }) {
                return Err("cannot register native replay window".into());
            }
            Ok(Self(checked(CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("OpenSporeNativeReplay"),
                w!("OpenSpore native replay"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            ))?))
        }
    }
}
impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

pub struct Renderer {
    device: IDirect3DDevice9,
    initial: Option<IDirect3DStateBlock9>,
    presentation: D3DPRESENT_PARAMETERS,
    pub adapter: Adapter,
    _api: IDirect3D9,
    _window: HiddenWindow,
}
#[derive(Debug, Serialize)]
pub struct Readback {
    pub width: u32,
    pub height: u32,
    pub format: i32,
    pub row_pitch: usize,
    pub bytes: Vec<u8>,
}
impl Readback {
    pub fn rgba8(&self) -> Result<Vec<u8>> {
        if self.format != 21 {
            return Err("RGBA8 export requires native A8R8G8B8 target".into());
        }
        Ok(self
            .bytes
            .chunks_exact(4)
            .flat_map(|v| [v[2], v[1], v[0], v[3]])
            .collect())
    }
}
enum GpuTexture {
    Flat(IDirect3DTexture9),
    Cube(IDirect3DCubeTexture9),
}
struct GpuTarget {
    texture: Option<IDirect3DTexture9>,
    surface: IDirect3DSurface9,
    depth: Option<IDirect3DSurface9>,
}

impl Renderer {
    pub fn new(ordinal: u32) -> Result<Self> {
        let adapter = adapters()?
            .into_iter()
            .find(|a| a.ordinal == ordinal)
            .ok_or("requested HAL adapter unavailable")?;
        let window = HiddenWindow::new()?;
        unsafe {
            let api = required(Direct3DCreate9(D3D_SDK_VERSION))?;
            let mut display = D3DDISPLAYMODE::default();
            checked(api.GetAdapterDisplayMode(ordinal, &mut display))?;
            let mut presentation = D3DPRESENT_PARAMETERS {
                BackBufferWidth: 1,
                BackBufferHeight: 1,
                BackBufferFormat: display.Format,
                BackBufferCount: 1,
                SwapEffect: D3DSWAPEFFECT_DISCARD,
                hDeviceWindow: window.0,
                Windowed: true.into(),
                PresentationInterval: D3DPRESENT_INTERVAL_IMMEDIATE as u32,
                ..Default::default()
            };
            let mut device = None;
            checked(api.CreateDevice(
                ordinal,
                D3DDEVTYPE_HAL,
                window.0,
                (D3DCREATE_SOFTWARE_VERTEXPROCESSING | D3DCREATE_FPU_PRESERVE) as u32,
                &mut presentation,
                &mut device,
            ))?;
            let device = required(device)?;
            let initial = Some(checked(device.CreateStateBlock(D3DSBT_ALL))?);
            Ok(Self {
                device,
                initial,
                presentation,
                adapter,
                _api: api,
                _window: window,
            })
        }
    }
    /// Releases all default-pool replay resources before Reset; replay uploads them afresh.
    /// Also used for deterministic resize/reset regression. Device-loss recovery must wait
    /// until TestCooperativeLevel permits Reset; errors propagate to the caller.
    pub fn reset(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("invalid reset dimensions".into());
        }
        unsafe {
            match self.device.TestCooperativeLevel() {
                Ok(()) => {
                    if self.initial.is_some() {
                        self.restore()?;
                    }
                }
                // D3DERR_DEVICENOTRESET: release the saved state and allow Reset.
                // DEVICELOST and all other failures remain explicit/retryable.
                Err(error) if error.code().0 == 0x88760869u32 as i32 => {}
                Err(error) => return Err(format!("D3D9 reset not ready: {error}")),
            }
            self.initial.take();
            self.presentation.BackBufferWidth = width;
            self.presentation.BackBufferHeight = height;
            checked(self.device.Reset(&mut self.presentation))?;
            self.initial = Some(checked(self.device.CreateStateBlock(D3DSBT_ALL))?);
        }
        Ok(())
    }
    unsafe fn restore(&self) -> Result<()> {
        checked(required(self.initial.as_ref().cloned())?.Apply())?;
        let back = checked(self.device.GetBackBuffer(0, 0, D3DBACKBUFFER_TYPE_MONO))?;
        checked(self.device.SetRenderTarget(0, &back))?;
        checked(self.device.SetDepthStencilSurface(None))?;
        Ok(())
    }
    pub fn replay(&self, packet: &NativeRenderPacket) -> Result<Vec<Readback>> {
        packet.validate()?;
        unsafe {
            self.restore()?;
            let result = self.replay_inner(packet);
            // Remove device-owned references even when a command failed.
            let cleanup = self.restore();
            match result {
                Ok(result) => {
                    cleanup?;
                    Ok(result)
                }
                Err(e) => Err(e),
            }
        }
    }
    unsafe fn replay_inner(&self, packet: &NativeRenderPacket) -> Result<Vec<Readback>> {
        let device = &self.device;
        let mut targets = vec![];
        for desc in &packet.targets {
            let mut texture = None;
            let surface = if desc.surface_only {
                let mut surface = None;
                checked(device.CreateRenderTarget(
                    desc.width,
                    desc.height,
                    D3DFORMAT(desc.format as u32),
                    D3DMULTISAMPLE_NONE,
                    0,
                    false,
                    &mut surface,
                    std::ptr::null_mut(),
                ))?;
                required(surface)?
            } else {
                checked(device.CreateTexture(
                    desc.width,
                    desc.height,
                    1,
                    D3DUSAGE_RENDERTARGET as u32,
                    D3DFORMAT(desc.format as u32),
                    D3DPOOL_DEFAULT,
                    &mut texture,
                    std::ptr::null_mut(),
                ))?;
                checked(
                    texture
                        .as_ref()
                        .ok_or("target texture missing")?
                        .GetSurfaceLevel(0),
                )?
            };
            let mut depth = None;
            if desc.depth_format != 0 {
                checked(device.CreateDepthStencilSurface(
                    desc.width,
                    desc.height,
                    D3DFORMAT(desc.depth_format as u32),
                    D3DMULTISAMPLE_NONE,
                    0,
                    true,
                    &mut depth,
                    std::ptr::null_mut(),
                ))?;
            }
            targets.push(GpuTarget {
                texture,
                surface,
                depth,
            });
        }
        let mut textures = vec![];
        for tex in &packet.textures {
            let levels = tex.faces[0].len() as u32;
            let gpu = if tex.faces.len() == 1 {
                let mut result = None;
                checked(device.CreateTexture(
                    tex.width,
                    tex.height,
                    levels,
                    0,
                    D3DFORMAT(tex.format as u32),
                    D3DPOOL_MANAGED,
                    &mut result,
                    std::ptr::null_mut(),
                ))?;
                GpuTexture::Flat(required(result)?)
            } else {
                let mut result = None;
                checked(device.CreateCubeTexture(
                    tex.width,
                    levels,
                    0,
                    D3DFORMAT(tex.format as u32),
                    D3DPOOL_MANAGED,
                    &mut result,
                    std::ptr::null_mut(),
                ))?;
                GpuTexture::Cube(required(result)?)
            };
            for (face, mips) in tex.faces.iter().enumerate() {
                for (level, mip) in mips.iter().enumerate() {
                    let mut lock = D3DLOCKED_RECT::default();
                    match &gpu {
                        GpuTexture::Flat(t) => {
                            checked(t.LockRect(level as u32, &mut lock, std::ptr::null(), 0))?
                        }
                        GpuTexture::Cube(t) => checked(t.LockRect(
                            D3DCUBEMAP_FACES(face as i32),
                            level as u32,
                            &mut lock,
                            std::ptr::null(),
                            0,
                        ))?,
                    }
                    let (row, rows) = surface_layout(
                        tex.format,
                        (tex.width >> level).max(1),
                        (tex.height >> level).max(1),
                    )?;
                    let upload = copy_to_lock(&lock, &mip.bytes, mip.row_pitch, row, rows);
                    let unlock = match &gpu {
                        GpuTexture::Flat(t) => t.UnlockRect(level as u32),
                        GpuTexture::Cube(t) => {
                            t.UnlockRect(D3DCUBEMAP_FACES(face as i32), level as u32)
                        }
                    };
                    upload?;
                    checked(unlock)?;
                }
            }
            textures.push(gpu);
        }
        let mut in_scene = false;
        let drawing: Result<()> = (|| {
            for command in &packet.commands {
                if !matches!(command, Command::Draw(_)) && in_scene {
                    checked(device.EndScene())?;
                    in_scene = false;
                }
                // Native copies run outside BeginScene/EndScene and use their
                // surface arguments, independently of current target/viewport.
                if let Command::ColorFill {
                    target,
                    rect,
                    color,
                } = command
                {
                    let rect = native_rect(*rect);
                    checked(device.ColorFill(
                        &targets[*target].surface,
                        rect.as_ref().map_or(std::ptr::null(), |r| r),
                        *color,
                    ))?;
                    continue;
                }
                if let Command::StretchRect {
                    source,
                    target,
                    source_rect,
                    target_rect,
                    filter,
                } = command
                {
                    let from = native_rect(*source_rect);
                    let to = native_rect(*target_rect);
                    checked(device.StretchRect(
                        &targets[*source].surface,
                        from.as_ref().map_or(std::ptr::null(), |r| r),
                        &targets[*target].surface,
                        to.as_ref().map_or(std::ptr::null(), |r| r),
                        D3DTEXTUREFILTERTYPE(*filter),
                    ))?;
                    continue;
                }
                let index = match command {
                    Command::Clear { target, .. } => *target,
                    Command::Draw(draw) => draw.target,
                    _ => unreachable!(),
                };
                let target = &targets[index];
                checked(device.SetDepthStencilSurface(None))?;
                checked(device.SetRenderTarget(0, &target.surface))?;
                checked(device.SetDepthStencilSurface(target.depth.as_ref()))?;
                match command {
                    Command::Clear {
                        flags,
                        color,
                        depth_bits,
                        stencil,
                        viewport,
                        rects,
                        scissor,
                        ..
                    } => {
                        self.viewport(viewport.unwrap_or([
                            0,
                            0,
                            packet.targets[index].width,
                            packet.targets[index].height,
                            0,
                            1.0f32.to_bits(),
                        ]))?;
                        self.scissor(*scissor)?;
                        let native_rects: Vec<_> = rects
                            .iter()
                            .map(|[x1, y1, x2, y2]| D3DRECT {
                                x1: *x1,
                                y1: *y1,
                                x2: *x2,
                                y2: *y2,
                            })
                            .collect();
                        checked(device.Clear(
                            native_rects.len() as u32,
                            if native_rects.is_empty() {
                                std::ptr::null()
                            } else {
                                native_rects.as_ptr()
                            },
                            *flags,
                            *color,
                            f32::from_bits(*depth_bits),
                            *stencil,
                        ))?;
                    }
                    Command::Draw(draw) => {
                        if !in_scene {
                            checked(device.BeginScene())?;
                            in_scene = true;
                        }
                        self.draw(draw, &textures, &targets)?;
                    }
                    _ => unreachable!(),
                }
            }
            Ok(())
        })();
        let end = if in_scene { device.EndScene() } else { Ok(()) };
        drawing?;
        checked(end)?;
        self.restore()?;
        let mut result = vec![];
        for (desc, target) in packet.targets.iter().zip(&targets) {
            let mut staging = None;
            checked(device.CreateOffscreenPlainSurface(
                desc.width,
                desc.height,
                D3DFORMAT(desc.format as u32),
                D3DPOOL_SYSTEMMEM,
                &mut staging,
                std::ptr::null_mut(),
            ))?;
            let staging = required(staging)?;
            checked(device.GetRenderTargetData(&target.surface, &staging))?;
            let mut lock = D3DLOCKED_RECT::default();
            checked(staging.LockRect(&mut lock, std::ptr::null(), D3DLOCK_READONLY as u32))?;
            let (row, rows) = surface_layout(desc.format, desc.width, desc.height)?;
            let pixels = copy_from_lock(&lock, row, rows);
            let unlock = staging.UnlockRect();
            let bytes = pixels?;
            checked(unlock)?;
            result.push(Readback {
                width: desc.width,
                height: desc.height,
                format: desc.format,
                row_pitch: row,
                bytes,
            });
        }
        Ok(result)
    }
    unsafe fn draw(
        &self,
        draw: &Draw,
        textures: &[GpuTexture],
        targets: &[GpuTarget],
    ) -> Result<()> {
        let device = &self.device;
        self.viewport(draw.viewport)?;
        let vs = draw
            .vertex_shader
            .as_ref()
            .map(|s| checked(device.CreateVertexShader(s.as_ptr())))
            .transpose()?;
        let ps = draw
            .pixel_shader
            .as_ref()
            .map(|s| checked(device.CreatePixelShader(s.as_ptr())))
            .transpose()?;
        checked(device.SetVertexShader(vs.as_ref()))?;
        checked(device.SetPixelShader(ps.as_ref()))?;
        let declaration;
        if let Some(fvf) = draw.fvf {
            checked(device.SetFVF(fvf))?;
        } else {
            let mut elements: Vec<_> = draw
                .declaration
                .iter()
                .map(|e| D3DVERTEXELEMENT9 {
                    Stream: e.stream,
                    Offset: e.offset,
                    Type: e.kind,
                    Method: e.method,
                    Usage: e.usage,
                    UsageIndex: e.usage_index,
                })
                .collect();
            elements.push(D3DVERTEXELEMENT9 {
                Stream: 0xff,
                Type: 17,
                ..Default::default()
            });
            declaration = checked(device.CreateVertexDeclaration(elements.as_ptr()))?;
            checked(device.SetVertexDeclaration(&declaration))?;
        }
        for [state, value] in &draw.render_states {
            checked(device.SetRenderState(D3DRENDERSTATETYPE(*state as i32), *value))?;
        }
        self.scissor(draw.scissor)?;
        for [sampler, state, value] in &draw.sampler_states {
            checked(device.SetSamplerState(*sampler, D3DSAMPLERSTATETYPE(*state as i32), *value))?;
        }
        for [stage, state, value] in &draw.texture_stage_states {
            checked(device.SetTextureStageState(
                *stage,
                D3DTEXTURESTAGESTATETYPE(*state as i32),
                *value,
            ))?;
        }
        for binding in &draw.bindings {
            match &binding.source {
                None => checked(device.SetTexture(binding.sampler, None))?,
                Some(TextureSource::Target(i)) => checked(
                    device.SetTexture(
                        binding.sampler,
                        targets[*i]
                            .texture
                            .as_ref()
                            .ok_or("standalone surface cannot be sampled")?,
                    ),
                )?,
                Some(TextureSource::Texture(i)) => match &textures[*i] {
                    GpuTexture::Flat(t) => checked(device.SetTexture(binding.sampler, t))?,
                    GpuTexture::Cube(t) => checked(device.SetTexture(binding.sampler, t))?,
                },
            }
        }
        for constant in &draw.constants {
            match constant {
                Constants::Float {
                    vertex,
                    start,
                    bits,
                } => {
                    let values: Vec<f32> =
                        bits.iter().flatten().map(|v| f32::from_bits(*v)).collect();
                    if *vertex {
                        checked(device.SetVertexShaderConstantF(
                            *start,
                            values.as_ptr(),
                            bits.len() as u32,
                        ))?;
                    } else {
                        checked(device.SetPixelShaderConstantF(
                            *start,
                            values.as_ptr(),
                            bits.len() as u32,
                        ))?;
                    }
                }
                Constants::Int {
                    vertex,
                    start,
                    values,
                } => {
                    let flat: Vec<i32> = values.iter().flatten().copied().collect();
                    if *vertex {
                        checked(device.SetVertexShaderConstantI(
                            *start,
                            flat.as_ptr(),
                            values.len() as u32,
                        ))?;
                    } else {
                        checked(device.SetPixelShaderConstantI(
                            *start,
                            flat.as_ptr(),
                            values.len() as u32,
                        ))?;
                    }
                }
                Constants::Bool {
                    vertex,
                    start,
                    values,
                } => {
                    let flat: Vec<windows::core::BOOL> =
                        values.iter().map(|v| (*v).into()).collect();
                    if *vertex {
                        checked(device.SetVertexShaderConstantB(
                            *start,
                            flat.as_ptr(),
                            values.len() as u32,
                        ))?;
                    } else {
                        checked(device.SetPixelShaderConstantB(
                            *start,
                            flat.as_ptr(),
                            values.len() as u32,
                        ))?;
                    }
                }
            }
        }
        if let Some(indices) = &draw.indices {
            if let Some(range) = &draw.indexed_range {
                return self.indexed_slice(draw, indices, range);
            }
            checked(device.DrawIndexedPrimitiveUP(
                D3DPRIMITIVETYPE(draw.primitive_type),
                0,
                draw.vertex_count,
                draw.primitive_count,
                indices.bytes.as_ptr().cast(),
                if indices.bits == 16 {
                    D3DFMT_INDEX16
                } else {
                    D3DFMT_INDEX32
                },
                draw.vertices.as_ptr().cast(),
                draw.stride,
            ))?;
        } else {
            checked(device.DrawPrimitiveUP(
                D3DPRIMITIVETYPE(draw.primitive_type),
                draw.primitive_count,
                draw.vertices.as_ptr().cast(),
                draw.stride,
            ))?;
        }
        Ok(())
    }
    unsafe fn indexed_slice(
        &self,
        draw: &Draw,
        indices: &IndexData,
        range: &IndexedRange,
    ) -> Result<()> {
        let device = &self.device;
        let mut vb = None;
        checked(device.CreateVertexBuffer(
            draw.vertices.len() as u32,
            D3DUSAGE_WRITEONLY as u32,
            0,
            D3DPOOL_DEFAULT,
            &mut vb,
            std::ptr::null_mut(),
        ))?;
        let vb = required(vb)?;
        let mut bytes = std::ptr::null_mut();
        checked(vb.Lock(0, draw.vertices.len() as u32, &mut bytes, 0))?;
        if bytes.is_null() {
            let _ = vb.Unlock();
            return Err("D3D9 vertex buffer lock returned null".into());
        }
        std::ptr::copy_nonoverlapping(draw.vertices.as_ptr(), bytes.cast(), draw.vertices.len());
        checked(vb.Unlock())?;
        let mut ib = None;
        checked(device.CreateIndexBuffer(
            indices.bytes.len() as u32,
            D3DUSAGE_WRITEONLY as u32,
            if indices.bits == 16 {
                D3DFMT_INDEX16
            } else {
                D3DFMT_INDEX32
            },
            D3DPOOL_DEFAULT,
            &mut ib,
            std::ptr::null_mut(),
        ))?;
        let ib = required(ib)?;
        checked(ib.Lock(0, indices.bytes.len() as u32, &mut bytes, 0))?;
        if bytes.is_null() {
            let _ = ib.Unlock();
            return Err("D3D9 index buffer lock returned null".into());
        }
        std::ptr::copy_nonoverlapping(indices.bytes.as_ptr(), bytes.cast(), indices.bytes.len());
        checked(ib.Unlock())?;
        let result = (|| {
            checked(device.SetStreamSourceFreq(0, 1))?;
            checked(device.SetStreamSource(0, &vb, 0, draw.stride))?;
            checked(device.SetIndices(&ib))?;
            // Rebase the resource binding, not the vertices or index payload.
            // base - origin + original_index selects exactly the captured byte
            // at original_index - min. No unobserved prefix is zero-filled.
            checked(device.DrawIndexedPrimitive(
                D3DPRIMITIVETYPE(draw.primitive_type),
                range.replay_base()?,
                range.min_vertex_index,
                range.num_vertices,
                0,
                draw.primitive_count,
            ))
        })();
        // Release device-held COM references on both success and failure. UP
        // source draws also leave these bindings null in the original API.
        let clear_indices = device.SetIndices(None);
        let clear_stream = device.SetStreamSource(0, None, 0, 0);
        result?;
        checked(clear_indices)?;
        checked(clear_stream)?;
        Ok(())
    }
    unsafe fn viewport(&self, [x, y, width, height, min, max]: [u32; 6]) -> Result<()> {
        checked(self.device.SetViewport(&D3DVIEWPORT9 {
            X: x,
            Y: y,
            Width: width,
            Height: height,
            MinZ: f32::from_bits(min),
            MaxZ: f32::from_bits(max),
        }))
    }
    unsafe fn scissor(&self, rect: Option<[i32; 4]>) -> Result<()> {
        if let Some([left, top, right, bottom]) = rect {
            checked(self.device.SetScissorRect(&RECT {
                left,
                top,
                right,
                bottom,
            }))?;
        }
        checked(
            self.device
                .SetRenderState(D3DRS_SCISSORTESTENABLE, u32::from(rect.is_some())),
        )
    }
}
unsafe fn copy_to_lock(
    lock: &D3DLOCKED_RECT,
    bytes: &[u8],
    pitch: usize,
    row: usize,
    rows: usize,
) -> Result<()> {
    if lock.pBits.is_null() || lock.Pitch < row as i32 {
        return Err("invalid D3D9 texture lock pitch".into());
    }
    for y in 0..rows {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr().add(y * pitch),
            lock.pBits.cast::<u8>().add(y * lock.Pitch as usize),
            row,
        );
    }
    Ok(())
}
fn native_rect(rect: Option<[i32; 4]>) -> Option<RECT> {
    rect.map(|[left, top, right, bottom]| RECT {
        left,
        top,
        right,
        bottom,
    })
}
unsafe fn copy_from_lock(lock: &D3DLOCKED_RECT, row: usize, rows: usize) -> Result<Vec<u8>> {
    if lock.pBits.is_null() || lock.Pitch < row as i32 {
        return Err("invalid D3D9 readback pitch".into());
    }
    let mut bytes = vec![0; row * rows];
    for y in 0..rows {
        std::ptr::copy_nonoverlapping(
            lock.pBits.cast::<u8>().add(y * lock.Pitch as usize),
            bytes.as_mut_ptr().add(y * row),
            row,
        );
    }
    Ok(bytes)
}
