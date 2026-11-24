//! Shadow-continuation tiers (`strict` / `normal` / `fast` + custom combinations)
//! per `DESIGN-skipstop-tiers-ebab8-1124.md` r3 §12.
//!
//! - Phase 1 ships `observe` + `strict` (`main_exact`) + `normal`: any un-signed relaxation
//!   (including `fast` or overrides looser than `normal`) falls back to `strict` (`main_exact`).
//! - Every configuration expands into all 8 fields; path rules resolve conflicts by taking the
//!   stricter setting per field.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Strict,
    Normal,
    Fast,
}

impl Tier {
    pub fn from_word(s: &str) -> Option<Self> {
        match s {
            "strict" => Some(Self::Strict),
            "normal" => Some(Self::Normal),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Normal => "normal",
            Self::Fast => "fast",
        }
    }
}

/// Ordered from strictest (`Off`) to loosest (`Scope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReuseMode {
    Off,
    Observe,
    MainExact,
    Exact,
    Scope,
}

impl ReuseMode {
    pub fn from_word(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "observe" => Some(Self::Observe),
            "main_exact" => Some(Self::MainExact),
            "exact" => Some(Self::Exact),
            "scope" => Some(Self::Scope),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::MainExact => "main_exact",
            Self::Exact => "exact",
            Self::Scope => "scope",
        }
    }
}

/// Ordered from strictest (`All`) to loosest (`Changed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KnifeScope {
    All,
    Affected,
    Changed,
}

impl KnifeScope {
    pub fn from_word(s: &str) -> Option<Self> {
        match s {
            "all" => Some(Self::All),
            "affected" => Some(Self::Affected),
            "changed" => Some(Self::Changed),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Affected => "affected",
            Self::Changed => "changed",
        }
    }
}

/// Ordered from strictest (`Both`) to loosest (`LinuxOnly`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlatformScope {
    Both,
    Native,
    LinuxOnly,
}

impl PlatformScope {
    pub fn from_word(s: &str) -> Option<Self> {
        match s {
            "both" => Some(Self::Both),
            "native" => Some(Self::Native),
            "linux_only" => Some(Self::LinuxOnly),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Native => "native",
            Self::LinuxOnly => "linux_only",
        }
    }
}

/// Ordered from strictest (`Reject`) to loosest (`Warn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StopMode {
    Reject,
    Warn,
}

impl StopMode {
    pub fn from_word(s: &str) -> Option<Self> {
        match s {
            "reject" => Some(Self::Reject),
            "warn" => Some(Self::Warn),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Warn => "warn",
        }
    }
}

/// Ordered from strictest (`AddCell`) to loosest (`Trim`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverflowMode {
    AddCell,
    Trim,
}

impl OverflowMode {
    pub fn from_word(s: &str) -> Option<Self> {
        match s {
            "add_cell" => Some(Self::AddCell),
            "trim" => Some(Self::Trim),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::AddCell => "add_cell",
            Self::Trim => "trim",
        }
    }
}

pub const CELL_MAX_MIN_RANGE: (u32, u32) = (10, 140);
pub const SCRIPT_MAX_KNIVES_RANGE: (u32, u32) = (20, 300);
pub const SCRIPT_MAX_MIN_RANGE: (u32, u32) = (5, 60);
pub const SPOT_EVERY_RANGE: (u32, u32) = (0, 100);

/// Complete 8-field expanded policy (plus `overflow` for cell budget handling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierPolicy {
    pub reuse: ReuseMode,
    /// Spot-check denominator (e.g. `5` = 1/5, `10` = 1/10; `0` = never spot-check).
    pub spot_every: u32,
    pub knives: KnifeScope,
    pub platforms: PlatformScope,
    pub stop: StopMode,
    pub cell_max_min: u32,
    pub script_max_knives: u32,
    pub script_max_min: u32,
    pub overflow: OverflowMode,
}

impl TierPolicy {
    pub fn for_tier(tier: Tier) -> Self {
        match tier {
            Tier::Strict => Self {
                reuse: ReuseMode::MainExact,
                spot_every: 5,
                knives: KnifeScope::All,
                platforms: PlatformScope::Both,
                stop: StopMode::Reject,
                cell_max_min: 60,
                script_max_knives: 100,
                script_max_min: 20,
                overflow: OverflowMode::AddCell,
            },
            Tier::Normal => Self {
                reuse: ReuseMode::MainExact,
                spot_every: 10,
                knives: KnifeScope::Affected,
                platforms: PlatformScope::Native,
                stop: StopMode::Reject,
                cell_max_min: 90,
                script_max_knives: 140,
                script_max_min: 25,
                overflow: OverflowMode::AddCell,
            },
            Tier::Fast => Self {
                reuse: ReuseMode::Scope,
                spot_every: 20,
                knives: KnifeScope::Changed,
                platforms: PlatformScope::LinuxOnly,
                stop: StopMode::Warn,
                cell_max_min: 60,
                script_max_knives: 200,
                script_max_min: 30,
                overflow: OverflowMode::Trim,
            },
        }
    }

    /// Take the stricter value of each field when two path rules conflict.
    pub fn stricter(&self, other: &Self) -> Self {
        Self {
            reuse: self.reuse.min(other.reuse),
            spot_every: match (self.spot_every, other.spot_every) {
                (0, 0) => 0,
                (0, n) | (n, 0) => n,
                (a, b) => a.min(b),
            },
            knives: self.knives.min(other.knives),
            platforms: self.platforms.min(other.platforms),
            stop: self.stop.min(other.stop),
            // Shorter cell cap only loosens when `Trim` is active; under `AddCell`, smaller cells are stricter.
            cell_max_min: match (self.overflow, other.overflow) {
                (OverflowMode::AddCell, OverflowMode::AddCell) => {
                    self.cell_max_min.min(other.cell_max_min)
                }
                _ => self.cell_max_min.max(other.cell_max_min),
            },
            script_max_knives: self.script_max_knives.min(other.script_max_knives),
            script_max_min: self.script_max_min.min(other.script_max_min),
            overflow: self.overflow.min(other.overflow),
        }
    }
}

/// Partial field overrides (used by `"overrides"` and `"paths"`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TierOverrides {
    pub reuse: Option<ReuseMode>,
    pub spot_every: Option<u32>,
    pub knives: Option<KnifeScope>,
    pub platforms: Option<PlatformScope>,
    pub stop: Option<StopMode>,
    pub cell_max_min: Option<u32>,
    pub script_max_knives: Option<u32>,
    pub script_max_min: Option<u32>,
    pub overflow: Option<OverflowMode>,
}

impl TierOverrides {
    pub fn apply(&self, base: &TierPolicy) -> TierPolicy {
        let mut p = base.clone();
        if let Some(v) = self.reuse {
            p.reuse = v;
        }
        if let Some(v) = self.spot_every {
            p.spot_every = v;
        }
        if let Some(v) = self.knives {
            p.knives = v;
        }
        if let Some(v) = self.platforms {
            p.platforms = v;
        }
        if let Some(v) = self.stop {
            p.stop = v;
        }
        if let Some(v) = self.cell_max_min {
            p.cell_max_min = v;
        }
        if let Some(v) = self.script_max_knives {
            p.script_max_knives = v;
        }
        if let Some(v) = self.script_max_min {
            p.script_max_min = v;
        }
        if let Some(v) = self.overflow {
            p.overflow = v;
        }
        p
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTierRule {
    pub glob: String,
    pub tier: Option<Tier>,
    pub overrides: TierOverrides,
}

/// Match a repo-relative path against a simple glob (`**` = any segments, `*` = one segment).
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let f: Vec<&str> = path.trim_matches('/').split('/').collect();
    glob_parts(&p, &f)
}

fn glob_parts(p: &[&str], f: &[&str]) -> bool {
    match (p.first(), f.first()) {
        (None, None) => true,
        (Some(&"**"), _) => glob_parts(&p[1..], f) || (!f.is_empty() && glob_parts(p, &f[1..])),
        (Some(pat), Some(seg)) => seg_match(pat, seg) && glob_parts(&p[1..], &f[1..]),
        _ => false,
    }
}

fn seg_match(pat: &str, seg: &str) -> bool {
    if pat == "*" {
        return true;
    }
    if let Some((pre, suf)) = pat.split_once('*') {
        seg.len() >= pre.len() + suf.len() && seg.starts_with(pre) && seg.ends_with(suf)
    } else {
        pat == seg
    }
}

/// Returns `true` if `next` relaxes ANY field compared to `current` (DESIGN r3 §12.5).
pub fn is_relaxation(current: &TierPolicy, next: &TierPolicy) -> bool {
    if next.reuse > current.reuse {
        return true;
    }
    // Spot-check becomes sparser when reuse is active and denominator grows or drops to 0.
    let reuses = !matches!(next.reuse, ReuseMode::Off | ReuseMode::Observe);
    if reuses {
        match (current.spot_every, next.spot_every) {
            (0, 0) => {}
            (_, 0) => return true,
            (c, n) if c > 0 && n > c => return true,
            _ => {}
        }
    }
    if next.knives > current.knives
        || next.platforms > current.platforms
        || next.stop > current.stop
        || next.overflow > current.overflow
        || next.script_max_knives > current.script_max_knives
        || next.script_max_min > current.script_max_min
    {
        return true;
    }
    if next.overflow == OverflowMode::Trim && next.cell_max_min < current.cell_max_min {
        return true;
    }
    false
}

/// Phase 1 gate (DESIGN r3 §12.6): any relaxation beyond `Normal` without a verified hardware
/// signature falls back to `Tier::Strict` (`main_exact`).
pub fn enforce_phase1(policy: &TierPolicy, has_hardware_sig: bool) -> (TierPolicy, bool) {
    if has_hardware_sig {
        return (policy.clone(), false);
    }
    let normal = TierPolicy::for_tier(Tier::Normal);
    if is_relaxation(&normal, policy) {
        (TierPolicy::for_tier(Tier::Strict), true)
    } else {
        (policy.clone(), false)
    }
}

/// Result label per DESIGN r3 §12.2–§12.3: only `deferred` counts as conditional pass; any
/// failure, timeout, or cancellation is `Fail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictLabel {
    StrictPass,
    NormalPass,
    ConditionalPass,
    Fail,
}

impl VerdictLabel {
    pub fn word_zh(self) -> &'static str {
        match self {
            Self::StrictPass => "嚴格通過",
            Self::NormalPass => "一般通過",
            Self::ConditionalPass => "條件通過",
            Self::Fail => "失敗",
        }
    }
}

pub fn classify_verdict(
    effective_tier: Tier,
    failed: usize,
    timed_out: usize,
    cancelled: usize,
    deferred: usize,
) -> VerdictLabel {
    if failed > 0 || timed_out > 0 || cancelled > 0 {
        return VerdictLabel::Fail;
    }
    if deferred > 0 {
        return VerdictLabel::ConditionalPass;
    }
    match effective_tier {
        Tier::Strict => VerdictLabel::StrictPass,
        Tier::Normal | Tier::Fast => VerdictLabel::NormalPass,
    }
}

/// Three repo-wide settings (DESIGN r3 §4 & §12.9):
/// platform-independent knives on Linux, knife script length check, and merge batch schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSettings {
    pub linux_knives: bool,
    pub knife_length_check: StopMode,
    pub merge_batch_tz: String,
    pub merge_batch_hours: Vec<u32>,
}

impl Default for RepoSettings {
    fn default() -> Self {
        Self {
            linux_knives: false,
            knife_length_check: StopMode::Reject,
            merge_batch_tz: "Asia/Taipei".to_string(),
            merge_batch_hours: vec![10, 16, 22],
        }
    }
}

pub fn enforce_repo_phase1(r: &RepoSettings, has_hardware_sig: bool) -> (RepoSettings, bool) {
    if has_hardware_sig || (!r.linux_knives && r.knife_length_check == StopMode::Reject) {
        return (r.clone(), false);
    }
    let mut safe = r.clone();
    safe.linux_knives = false;
    safe.knife_length_check = StopMode::Reject;
    (safe, true)
}

pub fn parse_repo_settings(v: &serde_json::Value) -> Result<RepoSettings, String> {
    let o = v.as_object().ok_or("\"repo_settings\" is not an object")?;
    let mut out = RepoSettings::default();
    for (k, val) in o {
        match k.as_str() {
            "linux_knives" => {
                out.linux_knives = val
                    .as_bool()
                    .ok_or("repo_settings: \"linux_knives\" is not a boolean")?;
            }
            "knife_length_check" => {
                let s = val
                    .as_str()
                    .ok_or("repo_settings: \"knife_length_check\" is not a string")?;
                out.knife_length_check = StopMode::from_word(s)
                    .ok_or_else(|| format!("repo_settings: \"{s}\" is not reject or warn"))?;
            }
            "merge_batch_tz" => {
                let s = val
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or("repo_settings: \"merge_batch_tz\" must be a non-empty string")?;
                out.merge_batch_tz = s.to_string();
            }
            "merge_batch_hours" => {
                let arr = val
                    .as_array()
                    .ok_or("repo_settings: \"merge_batch_hours\" is not a list")?;
                let mut hours = Vec::with_capacity(arr.len());
                for item in arr {
                    let h = item
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .filter(|&h| h <= 23)
                        .ok_or("repo_settings: \"merge_batch_hours\" entries must be 0..=23")?;
                    hours.push(h);
                }
                if hours.is_empty() {
                    return Err("repo_settings: \"merge_batch_hours\" cannot be empty".into());
                }
                out.merge_batch_hours = hours;
            }
            other => return Err(format!("repo_settings: \"{other}\" is not a setting")),
        }
    }
    Ok(out)
}

/// Parse `TierOverrides` from a JSON object, rejecting unknown keys and out-of-range numbers.
pub fn parse_overrides(v: &serde_json::Value, context: &str) -> Result<TierOverrides, String> {
    let o = v
        .as_object()
        .ok_or_else(|| format!("{context} is not an object"))?;
    let mut out = TierOverrides::default();
    let u32_range = |val: &serde_json::Value,
                     key: &str,
                     (lo, hi): (u32, u32)|
     -> Result<u32, String> {
        val.as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| (lo..=hi).contains(&n))
            .ok_or_else(|| format!("{context}: \"{key}\" must be a whole number from {lo} to {hi}"))
    };
    for (k, val) in o {
        match k.as_str() {
            "reuse" => {
                let s = val
                    .as_str()
                    .ok_or_else(|| format!("{context}: \"reuse\" is not a string"))?;
                out.reuse = Some(
                    ReuseMode::from_word(s)
                        .ok_or_else(|| format!("{context}: \"{s}\" is not a reuse mode"))?,
                );
            }
            "spot_every" => out.spot_every = Some(u32_range(val, k, SPOT_EVERY_RANGE)?),
            "knives" => {
                let s = val
                    .as_str()
                    .ok_or_else(|| format!("{context}: \"knives\" is not a string"))?;
                out.knives = Some(
                    KnifeScope::from_word(s)
                        .ok_or_else(|| format!("{context}: \"{s}\" is not a knife scope"))?,
                );
            }
            "platforms" => {
                let s = val
                    .as_str()
                    .ok_or_else(|| format!("{context}: \"platforms\" is not a string"))?;
                out.platforms = Some(
                    PlatformScope::from_word(s)
                        .ok_or_else(|| format!("{context}: \"{s}\" is not a platform scope"))?,
                );
            }
            "stop" => {
                let s = val
                    .as_str()
                    .ok_or_else(|| format!("{context}: \"stop\" is not a string"))?;
                out.stop = Some(
                    StopMode::from_word(s)
                        .ok_or_else(|| format!("{context}: \"{s}\" is not a stop mode"))?,
                );
            }
            "cell_max_min" => out.cell_max_min = Some(u32_range(val, k, CELL_MAX_MIN_RANGE)?),
            "script_max_knives" => {
                out.script_max_knives = Some(u32_range(val, k, SCRIPT_MAX_KNIVES_RANGE)?);
            }
            "script_max_min" => out.script_max_min = Some(u32_range(val, k, SCRIPT_MAX_MIN_RANGE)?),
            "overflow" => {
                let s = val
                    .as_str()
                    .ok_or_else(|| format!("{context}: \"overflow\" is not a string"))?;
                out.overflow = Some(
                    OverflowMode::from_word(s)
                        .ok_or_else(|| format!("{context}: \"{s}\" is not an overflow mode"))?,
                );
            }
            other => return Err(format!("{context}: \"{other}\" is not a tier field")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase1_falls_back_to_strict_on_any_unsigned_relaxation() {
        let strict = TierPolicy::for_tier(Tier::Strict);
        let normal = TierPolicy::for_tier(Tier::Normal);
        let fast = TierPolicy::for_tier(Tier::Fast);

        assert!(!is_relaxation(&normal, &strict));
        assert!(!is_relaxation(&normal, &normal));
        assert!(is_relaxation(&normal, &fast));

        assert_eq!(enforce_phase1(&strict, false), (strict.clone(), false));
        assert_eq!(enforce_phase1(&normal, false), (normal.clone(), false));
        // Un-signed fast or looser override falls back to strict
        assert_eq!(enforce_phase1(&fast, false), (strict.clone(), true));
        let mut loose = normal.clone();
        loose.script_max_knives = 141;
        assert_eq!(enforce_phase1(&loose, false), (strict.clone(), true));
        let mut loose_stop = normal.clone();
        loose_stop.stop = StopMode::Warn;
        assert_eq!(enforce_phase1(&loose_stop, false), (strict, true));
        // Signed fast is allowed
        assert_eq!(enforce_phase1(&fast, true), (fast, false));
    }

    #[test]
    fn conflicting_policies_pick_stricter_per_field() {
        let a = TierPolicy::for_tier(Tier::Normal);
        let b = TierPolicy::for_tier(Tier::Strict);
        assert_eq!(a.stricter(&b), b);
    }

    #[test]
    fn out_of_range_overrides_are_rejected_not_clamped() {
        for bad in [
            r#"{"cell_max_min": 9}"#,
            r#"{"cell_max_min": 141}"#,
            r#"{"script_max_knives": 19}"#,
            r#"{"script_max_knives": 301}"#,
            r#"{"script_max_min": 4}"#,
            r#"{"script_max_min": 61}"#,
            r#"{"unknown": 1}"#,
        ] {
            let v: serde_json::Value = serde_json::from_str(bad).unwrap();
            assert!(parse_overrides(&v, "overrides").is_err(), "{bad}");
        }
    }

    #[test]
    fn verdict_labels_treat_only_deferred_as_conditional_pass() {
        assert_eq!(
            classify_verdict(Tier::Strict, 0, 0, 0, 0),
            VerdictLabel::StrictPass
        );
        assert_eq!(
            classify_verdict(Tier::Normal, 0, 0, 0, 0),
            VerdictLabel::NormalPass
        );
        assert_eq!(
            classify_verdict(Tier::Normal, 0, 0, 0, 2),
            VerdictLabel::ConditionalPass
        );
        assert_eq!(
            classify_verdict(Tier::Strict, 1, 0, 0, 2),
            VerdictLabel::Fail
        );
        assert_eq!(
            classify_verdict(Tier::Normal, 0, 1, 0, 0),
            VerdictLabel::Fail
        );
        assert_eq!(
            classify_verdict(Tier::Normal, 0, 0, 1, 0),
            VerdictLabel::Fail
        );
    }
}
