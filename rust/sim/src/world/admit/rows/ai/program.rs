//! AiWorld's cold export order, including reads of identities returned by
//! earlier owners. NativeFacades is a host identity cache; native admission
//! consumes the generated Character births directly from their slab.
use super::*;
use crate::gen::*;
use crate::store::Strings;
use crate::world::admit::{AdmitError, Options};

fn invocation(
    export: &'static str,
    args: Vec<f64>,
    mut call: super::Call,
) -> (crate::world::admit::Stage, &'static str, super::Call) {
    call.args = args;
    (crate::world::admit::Stage::Ai, export, call)
}

/// The native sequence builds this in pieces. ai_world_begin interns voice
/// identities, and ai_world_roster interns Character names; the caller must
/// refresh its shared interner from those owners between the pieces.
pub struct AiRows {
    pub setup: AiWorldSetup,
    pub content: Option<AiWorldContent>,
    pub roster: Option<AiRosterSetup>,
    pub late: Option<AiWorldContent>,
    calls: super::super::Calls,
    roster_result: bool,
    activated: bool,
}

fn strings_since(call: &mut super::Call, strings: &Strings, before: usize) {
    call.strings = strings.list[before..].to_vec();
}

/// setupRows and new AiWorld. The setup block is paired only with begin;
/// descriptor reads and indexed owner queries have no new callArgs block.
pub fn begin(
    options: &Options,
    player_team: f64,
    mission_calls: &[bfe_content::value::Value],
    facts: &SetupFacts,
    assets: &AiAssets,
    strings: &mut Strings,
) -> Result<AiRows, AdmitError> {
    let before = strings.list.len();
    let setup = setup_rows(strings, options, player_team, mission_calls, facts, assets)?;
    let mut call = encode(AiWorldSetup::WORDS, |row, heap| setup.put(row, heap));
    strings_since(&mut call, strings, before);
    let mut calls = vec![
        invocation("ai_world_begin", Vec::new(), call),
        invocation("ai_world_boundary", Vec::new(), Default::default()),
    ];
    for index in 0..facts.buildings {
        calls.push(invocation(
            "ai_world_prop_owner",
            vec![index as f64],
            Default::default(),
        ));
    }
    Ok(AiRows {
        setup,
        content: None,
        roster: None,
        late: None,
        calls,
        roster_result: false,
        activated: false,
    })
}

impl AiRows {
    /// Invoke after begin and prop_owner returned, using the shared string
    /// table including the voice identities begin created inside Rust.
    pub fn content(
        &mut self,
        facts: &ContentFacts<'_>,
        strings: &mut Strings,
    ) -> Result<(), AdmitError> {
        if self.content.is_some() {
            return Err(error("AI initial content was already built"));
        }
        let before = strings.list.len();
        let content = content_rows(strings, facts)?;
        let mut call = encoded_content(&content, facts)?;
        strings_since(&mut call, strings, before);
        self.calls
            .push(invocation("ai_world_content", Vec::new(), call));
        self.calls.push(invocation(
            "ai_world_weapon_policies",
            Vec::new(),
            Default::default(),
        ));
        self.content = Some(content);
        Ok(())
    }

    /// The roster is a separate admission. Its generated program consumes
    /// immutable post/hull policies already joined by the content export.
    pub fn roster(
        &mut self,
        options: &Options,
        player_team: f64,
        unit_scale: f64,
        calls: &[bfe_content::value::Value],
        mission: Option<&bfe_content::value::Value>,
        prepared: &[PreparedClass<'_>],
        pads: Vec<F4PadSetup>,
        strings: &mut Strings,
    ) -> Result<(), AdmitError> {
        if self.content.is_none() || self.roster.is_some() {
            return Err(error("AI roster requires initial content exactly once"));
        }
        let before = strings.list.len();
        let roster = roster_rows(
            strings,
            options,
            player_team,
            unit_scale,
            calls,
            mission,
            prepared,
            pads,
        )?;
        let mut call = encode(AiRosterSetup::WORDS, |row, heap| roster.put(row, heap));
        strings_since(&mut call, strings, before);
        self.calls
            .push(invocation("ai_world_roster", Vec::new(), call));
        self.roster = Some(roster);
        Ok(())
    }

    /// A successful roster returns count+1. Use its actual result rather
    /// than guessing how many capped armies or local units were born.
    pub fn characters(&mut self, result: u32) -> Result<(), AdmitError> {
        if self.roster.is_none() || self.roster_result || result == 0 {
            return Err(error("AI roster did not return an initial Character count"));
        }
        for index in 0..result - 1 {
            self.calls.push(invocation(
                "ai_world_roster_character",
                vec![index as f64],
                Default::default(),
            ));
        }
        self.roster_result = true;
        Ok(())
    }

    /// Inactive hero/player stock completes after roster admission. The
    /// caller refreshes its interner with new native Character names first.
    /// This content is a delta, never a second full definition snapshot.
    pub fn activation(
        &mut self,
        facts: &ContentFacts<'_>,
        strings: &mut Strings,
    ) -> Result<(), AdmitError> {
        if !self.roster_result || self.activated {
            return Err(error(
                "AI activation requires a completed roster exactly once",
            ));
        }
        if !facts.weapons.is_empty()
            || !facts.weapon_data.is_empty()
            || !facts.posts.is_empty()
            || !facts.hulls.is_empty()
            || !facts.props.is_empty()
        {
            return Err(error(
                "AI activation content may add only classes and items",
            ));
        }
        if !facts.classes.is_empty() || !facts.items.is_empty() {
            let before = strings.list.len();
            let content = content_rows(strings, facts)?;
            let mut call = encoded_content(&content, facts)?;
            strings_since(&mut call, strings, before);
            self.calls
                .push(invocation("ai_world_content", Vec::new(), call));
            self.calls.push(invocation(
                "ai_world_weapon_policies",
                Vec::new(),
                Default::default(),
            ));
            self.late = Some(content);
        }
        self.calls.push(invocation(
            "ai_world_activate",
            Vec::new(),
            Default::default(),
        ));
        self.activated = true;
        Ok(())
    }

    /// Ordered calls built so far. The sequence dispatches each new suffix
    /// before constructing the next piece against its updated string table.
    pub fn invocations(&self) -> &[(crate::world::admit::Stage, &'static str, super::Call)] {
        &self.calls
    }
}
