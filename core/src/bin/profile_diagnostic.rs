//! Read-only source-profile/model diagnostic; never chooses scanner algorithms.
use barcode_research_core::{
    experiment::Experiment, local_signal, multi_profile, profile, sampling::ImageView,
};
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert!(
        a.len() == 13 || a.len() == 17,
        "RGB WIDTH HEIGHT X0 Y0 X1 Y1 X2 Y2 X3 Y3 FRACTIONS [AXIS LO HI N]"
    );
    println!("{{\"diagnostic\":\"profile\",\"profileAperture\":{},\"identityPhase\":{},\"relativeReuse\":{},\"nativeSharpen\":{},\"guardRanking\":{},\"greenLuminance\":{}}}",cfg!(feature="experimental-profile-aperture"),cfg!(feature="experimental-identity-phase"),cfg!(feature="experimental-relative-reuse"),cfg!(feature="experimental-native-sharpen"),cfg!(feature="experimental-guard-ranking"),cfg!(feature="experimental-green-luminance"));
    let data = std::fs::read(&a[1]).unwrap();
    let w: usize = a[2].parse().unwrap();
    let h: usize = a[3].parse().unwrap();
    let im = ImageView::new(&data, w, h, 3, w * 3).unwrap();
    let q = std::array::from_fn(|i| [a[4 + i * 2].parse().unwrap(), a[5 + i * 2].parse().unwrap()]);
    let mut e = Experiment::default();
    let window = if a.len() == 17 {
        Some((
            a[13].parse::<usize>().unwrap(),
            a[14].parse::<f64>().unwrap(),
            a[15].parse::<f64>().unwrap(),
            a[16].parse::<usize>().unwrap(),
        ))
    } else {
        None
    };
    if let Some((axis, lo, hi, n)) = window {
        println!("{{\"axis\":{axis},\"lo\":{lo},\"hi\":{hi},\"samples\":{n}}}");
    }
    for fraction in a[12].split(',').map(|x| x.parse::<f64>().unwrap()) {
        for native in [false, true] {
            for interior in [false, true] {
                if window.is_some() && native {
                    continue;
                }
                let p = if let Some((axis, lo, hi, n)) = window {
                    e.diagnostic_segment(im, q, axis, fraction, lo, hi, n, interior)
                } else if interior {
                    e.diagnostic_interior_profile(im, q, 0, fraction, native)
                } else {
                    e.diagnostic_profile(im, q, 0, fraction, native)
                }
                .unwrap();
                let Some(p) = p else { continue };
                println!("{{\"fraction\":{},\"native\":{},\"interior\":{},\"signal\":{:?},\"local\":{:?}}}",fraction,native,interior,p,local_signal::normalize(&p).unwrap());
                for (name, result) in [
                    ("integer", multi_profile::decode_many(&p, 64)),
                    ("guard", multi_profile::decode_guard_bias(&p, 64)),
                    ("fractional", multi_profile::decode_fractional(&p, 64)),
                    (
                        "fractionalGuard",
                        multi_profile::decode_fractional_guard_bias(&p, 64),
                    ),
                    ("local", local_signal::decode(&p, 64)),
                    ("localGuard", local_signal::decode_guard_bias(&p, 64)),
                ] {
                    let r = result.unwrap();
                    for v in r.symbols {
                        let text: String = v.digits.iter().map(|d| char::from(b'0' + d)).collect();
                        println!("{{\"fraction\":{},\"native\":{},\"interior\":{},\"model\":\"{}\",\"text\":\"{}\",\"left\":{},\"right\":{},\"cost\":{},\"gap\":{}}}",fraction,native,interior,name,text,v.left,v.right,v.cost,v.gap);
                    }
                }
                if p.len() == 512 {
                    if let Some(v) = profile::decode(&p).unwrap() {
                        let text: String = v.digits.iter().map(|d| char::from(b'0' + d)).collect();
                        println!("{{\"fraction\":{},\"native\":{},\"interior\":{},\"model\":\"profile\",\"text\":\"{}\",\"left\":{},\"right\":{},\"cost\":{},\"gap\":{}}}",fraction,native,interior,text,v.left,v.right,v.cost,v.gap);
                    }
                }
            }
        }
    }
}
