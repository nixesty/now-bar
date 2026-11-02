//! One UI card material: the desktop behind the card, blurred by DWM, then saturated,
//! dimmed and lifted with a D2D ColorMatrix effect, masked to an anti-aliased rounded rect.
//!
//! Only this route works with the system compositor: shapes and mask brushes reject a backdrop
//! brush directly, geometric clips are aliased, and RectangleClip radii cannot be animated.

use std::cell::RefCell;

use windows::core::{h, implement, Interface, Result, GUID, HSTRING, PCWSTR};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl};
use windows::UI::Composition::{CompositionEffectSourceParameter, CompositionMaskBrush, CompositionNineGridBrush, CompositionStretch};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl, GRAPHICS_EFFECT_PROPERTY_MAPPING,
};

use super::gfx::{draw_surface, white, Gfx};

/// White lift fitted to One UI's Now Bar screenshots (black wallpaper → #121212). Saturation (2.0) and
/// brightness (0.65) are settings.
const LIFT: f32 = 0.07;

/// Radius of the nine-grid mask's source circle, in pixels. Its inset scales map it to any radius.
pub const MASK_R: f32 = 64.0;

/// Row-major D2D 5×4 colour matrix: rows r, g, b, a, offset.
pub fn one_ui_matrix() -> [f32; 20] {
    let s = crate::settings::current();
    let (saturation, dim) = (s.material_saturation_pct as f32 / 100.0, 1.0 - s.material_darkness_pct as f32 / 100.0);
    let lum = [0.2126f32, 0.7152, 0.0722];
    let mut m = [0.0f32; 20];
    for (row, l) in lum.iter().enumerate() {
        for col in 0..3 {
            let v = l * (1.0 - saturation) + if row == col { saturation } else { 0.0 };
            m[row * 4 + col] = v * dim;
        }
    }
    m[15] = 1.0;
    m[16] = LIFT;
    m[17] = LIFT;
    m[18] = LIFT;
    m
}

/// CLSID_D2D1ColorMatrix as an `IGraphicsEffect`, so no Win2D is needed.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct ColorMatrixEffect {
    name: RefCell<HSTRING>,
    matrix: [f32; 20],
    source: IGraphicsEffectSource,
}

impl IGraphicsEffect_Impl for ColorMatrixEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.borrow().clone())
    }
    fn SetName(&self, name: &HSTRING) -> Result<()> {
        *self.name.borrow_mut() = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for ColorMatrixEffect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for ColorMatrixEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(GUID::from_u128(0x921f03d6_641c_47df_852d_b4bb6153ae11))
    }
    fn GetNamedPropertyMapping(&self, _: &PCWSTR, _: *mut u32, _: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING) -> Result<()> {
        Err(E_INVALIDARG.into())
    }
    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }
    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingleArray(&self.matrix)?.cast(),
            1 => PropertyValue::CreateUInt32(1)?.cast(), // D2D1_COLORMATRIX_ALPHA_MODE_PREMULTIPLIED
            2 => PropertyValue::CreateBoolean(true)?.cast(), // clamp output
            _ => Err(E_INVALIDARG.into()),
        }
    }
    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }
    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
}

/// A brush that paints the One UI card material in its sprite's bounds, with anti-aliased rounded
/// corners. Bind the nine-grid's four `*InsetScale` properties to `radius_px / MASK_R`.
/// The window must have `DWMWA_USE_HOSTBACKDROPBRUSH` set.
pub fn blurred_card_brush(g: &Gfx) -> Result<(CompositionMaskBrush, CompositionNineGridBrush)> {
    let c = &g.compositor;
    let effect: IGraphicsEffect = ColorMatrixEffect {
        name: RefCell::new(HSTRING::new()),
        matrix: one_ui_matrix(),
        source: CompositionEffectSourceParameter::Create(h!("backdrop"))?.cast()?,
    }
    .into();
    let material = c.CreateEffectFactory(&effect)?.CreateBrush()?;
    material.SetSourceParameter(h!("backdrop"), &c.CreateHostBackdropBrush()?)?;

    let nine = rounded_mask(g)?;
    let masked = c.CreateMaskBrush()?;
    masked.SetSource(&material)?;
    masked.SetMask(&nine)?;
    Ok((masked, nine))
}

/// Nine-grid of an anti-aliased rounded square. The 2 px solid centre band is what stretches;
/// an empty centre makes the nine-grid collapse.
pub fn rounded_mask(g: &Gfx) -> Result<CompositionNineGridBrush> {
    let side = MASK_R * 2.0 + 2.0;
    let surface = draw_surface(g, side as i32, side as i32, |ink| {
        ink.rrect(super::gfx::rect(0.0, 0.0, side, side), MASK_R, white(1.0))
    })?;
    let source = g.compositor.CreateSurfaceBrushWithSurface(&surface)?;
    source.SetStretch(CompositionStretch::Fill)?;
    let nine = g.compositor.CreateNineGridBrush()?;
    nine.SetSource(&source)?;
    nine.SetInsets(MASK_R)?;
    Ok(nine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_values() {
        let m = one_ui_matrix();
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(m[0], 1.16181), "m[0] = {}", m[0]);
        assert!(close(m[1], -0.13819), "m[1] = {}", m[1]);
        assert!(close(m[5], 0.83512), "m[5] = {}", m[5]);
        assert_eq!(m[3], 0.0);
        assert_eq!(m[15], 1.0);
        assert_eq!((m[16], m[17], m[18]), (0.07, 0.07, 0.07));
    }
}
