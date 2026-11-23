//! Editable native analysis packets and isolated generated stems.
//! Only measurements cross into rendering; a packet contains no source PCM/path.
use crate::{analyze_with_texture, attack_guard, synthesize, Model, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub version: u32,
    pub model: Model,
    pub attacks: Vec<attack_guard::AttackEvidence>,
}

#[derive(Debug)]
pub struct Stems {
    pub tonal: Vec<f32>,
    pub texture: Vec<f32>,
}

impl Stems {
    pub fn mix(&self, controls: Controls) -> Result<Vec<f32>> {
        if self.tonal.len() != self.texture.len()
            || controls
                .gain_db
                .iter()
                .any(|g| !g.is_finite() || !(-60. ..=6.).contains(g))
            || controls.solo.is_some_and(|i| i > 1)
        {
            return Err("Invalid layer controls".into());
        }
        let gains: [f32; 2] = std::array::from_fn(|i| {
            if controls.muted[i] || controls.solo.is_some_and(|s| s != i) {
                0.
            } else {
                10_f32.powf(controls.gain_db[i] / 20.)
            }
        });
        Ok(self
            .tonal
            .iter()
            .zip(&self.texture)
            .map(|(a, b)| a * gains[0] + b * gains[1])
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    pub gain_db: [f32; 2],
    pub muted: [bool; 2],
    pub solo: Option<usize>,
}

pub fn analyze(input: &[f32], rate: u32) -> Result<Packet> {
    Ok(Packet {
        version: 1,
        model: analyze_with_texture(input, rate, true)?,
        attacks: attack_guard::measure_quiet_attacks(input, rate)?,
    })
}

/// Gains are computed once from the complete mix, then shared by all stems.
/// Solo/mute/gain never reruns normalization or changes another generated stem.
pub fn render(packet: &Packet, seed: u64, entry_shape: bool) -> Result<Stems> {
    if packet.version != 1 {
        return Err("Unsupported layer packet version".into());
    }
    // Validate the complete model before extracting independently generated layers.
    let _ = synthesize(&packet.model, seed)?;
    let mut tone_model = packet.model.clone();
    tone_model.texture.clear();
    let mut tonal = synthesize(&tone_model, seed)?;
    let mut texture = vec![0.; tonal.len()];
    if !packet.model.texture.is_empty() {
        crate::add_texture(&mut texture, &packet.model, seed);
    }
    let mut mix: Vec<_> = tonal.iter().zip(&texture).map(|(a, b)| a + b).collect();
    let guard = attack_guard::pre_attack_gain(&mix, &packet.attacks)?;
    for i in 0..mix.len() {
        tonal[i] *= guard[i];
        texture[i] *= guard[i];
        mix[i] = tonal[i] + texture[i];
    }
    if entry_shape {
        let shape = attack_guard::quiet_entry_gain(&mix, &packet.attacks)?;
        for i in 0..mix.len() {
            tonal[i] *= shape[i];
            texture[i] *= shape[i];
        }
    }
    Ok(Stems { tonal, texture })
}
