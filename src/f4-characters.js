// Character fields have one native owner after their construction handoff.
// AI planners submit scalar commands; standalone construction keeps ordinary
// fields until that handoff registers the facade.
const owners=new WeakMap();
export function bindCharacterOwner(character,owner){owners.set(character,owner);}
export function characterController(character,controller){
  const owner=owners.get(character);
  if(owner)return owner.characterController(character,controller);
  character.controller=controller;
}
export function characterObjective(character,post){
  const owner=owners.get(character);
  if(owner)return owner.objective(character,post);
  character.objective=post;
}

export function readyCharacter(character,choice){
  const owner=owners.get(character);
  if(!owner)return false;
  owner.readyHuman(character,choice);return true;
}
