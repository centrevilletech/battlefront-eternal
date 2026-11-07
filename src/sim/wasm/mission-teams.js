// Page spawn selection and HUD relationships need the same mission parser
// as native spawning. This cold reader creates metadata, never Characters or
// controllers; the authoritative roster can supply its current table instead.
import { bridgeFor } from '../runtime.js';
import { AiMissionTeamsInputLayout,putAiMissionTeamsInput,AiMissionTeamsMirror,syncAiMissionTeams } from '../gen/ai-phase.js';
const INPUT={words:AiMissionTeamsInputLayout.words,put:putAiMissionTeamsInput};
const OUTPUT={Mirror:AiMissionTeamsMirror,sync:syncAiMissionTeams};
export function missionTeams(instance,calls,{roster=false,names=[]}={}) {
  const bridge=bridgeFor(instance);
  bridge.callArgs(INPUT,{calls,names});
  const result=bridge.callResult(OUTPUT,bridge.x.ai_mission_teams(Number(roster)));
  return {relations:result.teams.map(team=>team.relations),factions:result.teams.map(team=>team.faction),
    capture:result.teams.map(team=>team.capture),names:result.names};
}
