#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grade {
    D,
    C,
    B,
    A,
    S,
    SPlus,
}

impl Grade {
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

const OVERPERFORM_CEIL: f64 = 1.2;

struct RoleProfile {
    kda: f64,
    kp: f64,
    cs_per_min: f64,
    damage_share: f64,
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

pub struct GradeInput<'a> {
    pub role: &'a str,
    pub kda: f64,
    pub kp: u8,
    pub cs_per_min: f64,
    pub damage_share: u8,
    pub vision_per_min: f64,
}

pub fn evaluate(input: &GradeInput<'_>) -> Grade {
    let p = profile(input.role);

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
