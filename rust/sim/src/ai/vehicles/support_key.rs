//! Numeric identities for support.js's `${mask}/${radius.toFixed(1)}` keys.
//! Formatting belongs to capture/publication; native lookups retain no text.
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Radius {
    Tenths { negative: bool, value: u128 },
    Number(u64),
}
fn bits(value: f64) -> u64 {
    if value == 0. {
        0
    } else if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        value.to_bits()
    }
}
impl Radius {
    fn new(value: f64) -> Self {
        let negative = value < 0.;
        let x = value.abs();
        if !x.is_finite() || x >= 1e21 {
            return Self::Number(bits(value));
        }
        // Keep binary multiplication exact. Multiplying the f64 by ten first
        // changes ties such as 1.05, which intentionally rounds down in V8.
        let raw = x.to_bits();
        let exp = ((raw >> 52) & 0x7ff) as i32;
        let frac = raw & ((1u64 << 52) - 1);
        let (mantissa, exponent) = if exp == 0 {
            (frac, -1074)
        } else {
            (frac | (1u64 << 52), exp - 1075)
        };
        let num = mantissa as u128 * 10;
        let value = if exponent >= 0 {
            num << exponent
        } else {
            let k = -exponent as u32;
            if k > 100 {
                0
            } else {
                let q = num >> k;
                let rem = num - (q << k);
                q + u128::from(rem * 2 >= (1u128 << k))
            }
        };
        Self::Tenths { negative, value }
    }
    fn parse(text: &str) -> Self {
        if let Some((whole, decimal)) = text
            .split_once('.')
            .filter(|(_, decimal)| decimal.len() == 1)
        {
            assert_eq!(decimal.len(), 1, "support radius decimal");
            let negative = whole.starts_with('-');
            let whole = whole
                .strip_prefix('-')
                .unwrap_or(whole)
                .parse::<u128>()
                .expect("support radius integer");
            let decimal = decimal.parse::<u128>().expect("support radius tenth");
            Self::Tenths {
                negative,
                value: whole * 10 + decimal,
            }
        } else {
            Self::Number(bits(bfe_content::js::string_to_number(text)))
        }
    }
}
impl fmt::Display for Radius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Tenths { negative, value } => write!(
                f,
                "{}{whole}.{decimal}",
                if negative { "-" } else { "" },
                whole = value / 10,
                decimal = value % 10
            ),
            Self::Number(raw) => {
                f.write_str(&bfe_content::js::number_to_string(f64::from_bits(raw)))
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupportKey {
    mask: u64,
    radius: Radius,
}
impl SupportKey {
    pub fn new(mask: f64, radius: f64) -> Self {
        Self {
            mask: bits(mask),
            radius: Radius::new(radius),
        }
    }
    /// Capture reader only; the decimal is already rounded, including -0.0.
    pub fn parse(text: &str) -> Self {
        let (mask, radius) = text.split_once('/').expect("support cache key");
        Self {
            mask: bits(bfe_content::js::string_to_number(mask)),
            radius: Radius::parse(radius),
        }
    }
}
impl fmt::Display for SupportKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}",
            bfe_content::js::number_to_string(f64::from_bits(self.mask)),
            self.radius
        )
    }
}
/// JavaScript Number.toFixed(1), used only by source twins and publications.
pub fn to_fixed_1(value: f64) -> String {
    Radius::new(value).to_string()
}
