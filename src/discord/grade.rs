//! Phase 4 of the HTML scorecard roadmap: the performance **grade** (S+/S/…/D).
//!
//! Riot exposes no "grade" field, so this is a self-contained scoring formula —
//! the roadmap flags it explicitly as a *product decision* (§4). It is a pure
//! function of stats we already derive, so it needs no API call, no DB and no
//! Chromium, and is unit-tested in isolation like the rest of the builder.
//!
//! ## Design
//!
//! The grade is a **role-relative** composite. Five components, each already
//! computed for the card, are scored against a per-role baseline (the value a
//! solid game hits) and blended with per-role weights:
//!
//! | Component      | Why role-relative                                    |
//! |----------------|------------------------------------------------------|
//! | KDA ratio      | universal, but supports live on a lower absolute KDA |
//! | Kill particip. | junglers/supports are expected to roam and assist    |
//! | CS / min       | supports barely farm; ADCs/mids should               |
//! | Damage share   | carries deal most of it, supports/tanks less         |
//! | Vision / min   | supports/junglers ward far more                      |
//!
//! Each component yields `actual / baseline`, clamped to [`OVERPERFORM_CEIL`] so
//! one monstrous stat can't alone buy an S+. The weighted blend (weights sum to
//! `1.0`) is then bucketed. A baseline-meeting game blends to ~`1.0` → low S;
//! beating baselines across the board reaches S+, and falling short drops to A/B
//! and below.
//!
//! **Objectives are deliberately excluded.** match-v5 gives no per-player
//! objective *participation*, only whole-team tallies (already shown in the team
//! panel), so folding them into an individual grade would credit a player for
//! their team's Barons. Kill participation already captures teamplay.

/// Performance bucket shown in the sidebar diamond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grade {
    // Ordered worst→best so derived `Ord` matches intuition (`D < … < SPlus`).
    D,
    C,
    B,
    A,
    S,
    SPlus,
}

impl Grade {
    /// The badge text (`S+`, `S`, `A`, …).
    pub fn label(self) -> &'static str {
        match self {
            Grade::SPlus => "S+",
            Grade::S => "S",
            Grade::A => "A",
            Grade::B => "B",
            Grade::C => "C",
            Grade::D => "D",
        }
    }
}

/// Ceiling on a single component's `actual / baseline` ratio. Caps how much one
/// dominant stat can lift the blend, so an S+ needs all-round excellence.
const OVERPERFORM_CEIL: f64 = 1.2;

/// Per-role baselines (a solid game's value) plus the blend weights. `weights`
/// sum to `1.0`, so a game that exactly meets every baseline scores `1.0`.
struct RoleProfile {
    kda: f64,
    kp: f64, // percent (0..100)
    cs_per_min: f64,
    damage_share: f64, // percent (0..100)
    vision_per_min: f64,
    weights: Weights,
}

struct Weights {
    kda: f64,
    kp: f64,
    cs: f64,
    damage: f64,
    vision: f64,
}

/// Baseline + weighting for a `team_position`. Empty position (ARAM / unknown)
/// falls through to a generic carry-ish profile.
fn profile(role: &str) -> RoleProfile {
    match role {
        "TOP" => RoleProfile {
            kda: 4.0,
            kp: 55.0,
            cs_per_min: 7.0,
            damage_share: 22.0,
            vision_per_min: 0.8,
            weights: Weights {
                kda: 0.30,
                kp: 0.15,
                cs: 0.20,
                damage: 0.25,
                vision: 0.10,
            },
        },
        "JUNGLE" => RoleProfile {
            kda: 4.0,
            kp: 65.0,
            cs_per_min: 5.5,
            damage_share: 18.0,
            vision_per_min: 1.2,
            weights: Weights {
                kda: 0.25,
                kp: 0.25,
                cs: 0.15,
                damage: 0.15,
                vision: 0.20,
            },
        },
        "MIDDLE" => RoleProfile {
            kda: 4.5,
            kp: 60.0,
            cs_per_min: 7.5,
            damage_share: 27.0,
            vision_per_min: 0.8,
            weights: Weights {
                kda: 0.28,
                kp: 0.18,
                cs: 0.19,
                damage: 0.27,
                vision: 0.08,
            },
        },
        "BOTTOM" => RoleProfile {
            kda: 4.5,
            kp: 58.0,
            cs_per_min: 8.0,
            damage_share: 28.0,
            vision_per_min: 0.7,
            weights: Weights {
                kda: 0.28,
                kp: 0.15,
                cs: 0.22,
                damage: 0.28,
                vision: 0.07,
            },
        },
        "UTILITY" => RoleProfile {
            kda: 3.5,
            kp: 68.0,
            cs_per_min: 1.5,
            damage_share: 12.0,
            vision_per_min: 1.8,
            weights: Weights {
                kda: 0.30,
                kp: 0.30,
                cs: 0.05,
                damage: 0.10,
                vision: 0.25,
            },
        },
        // ARAM / unknown: no lanes, so lean on fighting stats over farm/vision.
        _ => RoleProfile {
            kda: 4.0,
            kp: 60.0,
            cs_per_min: 6.0,
            damage_share: 25.0,
            vision_per_min: 0.6,
            weights: Weights {
                kda: 0.35,
                kp: 0.25,
                cs: 0.10,
                damage: 0.25,
                vision: 0.05,
            },
        },
    }
}

/// Already-derived stats fed into the grade. All are role-agnostic here; the
/// role selects the baseline they're measured against.
pub struct GradeInput<'a> {
    pub role: &'a str,
    pub kda: f64,
    pub kp: u8,
    pub cs_per_min: f64,
    pub damage_share: u8,
    pub vision_per_min: f64,
}

/// Score a performance into a [`Grade`].
pub fn evaluate(input: &GradeInput<'_>) -> Grade {
    let p = profile(input.role);

    // Ratio to baseline, floored at 0 and capped so one stat can't run away.
    let ratio = |actual: f64, baseline: f64| {
        if baseline <= 0.0 {
            OVERPERFORM_CEIL
        } else {
            (actual / baseline).clamp(0.0, OVERPERFORM_CEIL)
        }
    };

    let score = p.weights.kda * ratio(input.kda, p.kda)
        + p.weights.kp * ratio(input.kp as f64, p.kp)
        + p.weights.cs * ratio(input.cs_per_min, p.cs_per_min)
        + p.weights.damage * ratio(input.damage_share as f64, p.damage_share)
        + p.weights.vision * ratio(input.vision_per_min, p.vision_per_min);

    bucket(score)
}

/// Map a blended score (0..=[`OVERPERFORM_CEIL`]) to a bucket. Thresholds are
/// tuned so meeting baselines lands ~S, all-round overperformance reaches S+,
/// and shortfalls fall away through A/B/C to D.
fn bucket(score: f64) -> Grade {
    if score >= 1.02 {
        Grade::SPlus
    } else if score >= 0.90 {
        Grade::S
    } else if score >= 0.76 {
        Grade::A
    } else if score >= 0.60 {
        Grade::B
    } else if score >= 0.44 {
        Grade::C
    } else {
        Grade::D
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hard-carry jungle game (the design's S+ sample: 16/1/4, high KP, huge
    /// farm and damage) must reach the top bucket.
    #[test]
    fn dominant_game_is_s_plus() {
        let g = evaluate(&GradeInput {
            role: "JUNGLE",
            kda: 20.0,
            kp: 71,
            cs_per_min: 9.3,
            damage_share: 34,
            vision_per_min: 0.9,
        });
        assert_eq!(g, Grade::SPlus, "expected S+, got {}", g.label());
    }

    /// A thoroughly bad game floors out at D.
    #[test]
    fn feeding_game_is_d() {
        let g = evaluate(&GradeInput {
            role: "BOTTOM",
            kda: 0.3,
            kp: 20,
            cs_per_min: 3.0,
            damage_share: 8,
            vision_per_min: 0.2,
        });
        assert_eq!(g, Grade::D, "expected D, got {}", g.label());
    }

    /// Meeting every baseline exactly blends to ~1.0 → a clean S.
    #[test]
    fn baseline_game_is_s() {
        let g = evaluate(&GradeInput {
            role: "MIDDLE",
            kda: 4.5,
            kp: 60,
            cs_per_min: 7.5,
            damage_share: 27,
            vision_per_min: 0.8,
        });
        assert_eq!(g, Grade::S);
    }

    /// The grade is role-relative: a support with almost no CS and low damage is
    /// judged kindly, while the *same* stat line as an ADC is a poor game.
    #[test]
    fn role_relative_scoring() {
        let support = evaluate(&GradeInput {
            role: "UTILITY",
            kda: 4.0,
            kp: 70,
            cs_per_min: 1.6,
            damage_share: 12,
            vision_per_min: 2.0,
        });
        let adc_same = evaluate(&GradeInput {
            role: "BOTTOM",
            kda: 4.0,
            kp: 70,
            cs_per_min: 1.6,
            damage_share: 12,
            vision_per_min: 2.0,
        });
        assert!(
            support > adc_same,
            "support {} should outscore an ADC {} with the same low-farm line",
            support.label(),
            adc_same.label()
        );
    }

    /// Grades are monotonic in overall performance.
    #[test]
    fn better_stats_never_lower_the_grade() {
        let ok = evaluate(&GradeInput {
            role: "TOP",
            kda: 2.5,
            kp: 45,
            cs_per_min: 6.0,
            damage_share: 18,
            vision_per_min: 0.5,
        });
        let great = evaluate(&GradeInput {
            role: "TOP",
            kda: 6.0,
            kp: 65,
            cs_per_min: 8.5,
            damage_share: 30,
            vision_per_min: 1.0,
        });
        assert!(great >= ok);
        assert!(great >= Grade::S);
    }

    /// Empty position (ARAM) grades on the generic profile without panicking.
    #[test]
    fn aram_uses_generic_profile() {
        let g = evaluate(&GradeInput {
            role: "",
            kda: 5.0,
            kp: 62,
            cs_per_min: 6.5,
            damage_share: 26,
            vision_per_min: 0.6,
        });
        assert!(g >= Grade::A);
    }
}
