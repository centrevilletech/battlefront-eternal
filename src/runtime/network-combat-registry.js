// References come only from matched local content; wire messages carry IDs.
export function createCombatRegistry(data) {
  const paths=[], definitions=new Map(), ids=new WeakMap(), seen=new WeakSet();
  function visit(value,path){
    if(!value||typeof value!=='object'||seen.has(value)||ArrayBuffer.isView(value)||value instanceof ArrayBuffer)return;
    const proto=Object.getPrototypeOf(value);if(proto!==Object.prototype&&proto!==Array.prototype&&proto!==null)return;
    if(path.length>32)throw Error('combat registry content depth exceeded');
    seen.add(value);
    if(Array.isArray(value)&&value.length&&typeof value[0]!=='object')return;
    if(['explosion','droid','mine','powerupitem','grenade','sticky','missile','shell','bullet','beam','fatray'].includes(value.base)||Number.isFinite(value.Velocity)&&Number.isFinite(value.LifeSpan)||
      typeof value.Effect==='string'&&(Number.isFinite(value.Damage)||Number.isFinite(value.DamageRadiusOuter)||Number.isFinite(value.LightRadius))){
      if(paths.length>=65535)throw Error('combat definition registry capacity exceeded');
      const id=paths.length+1;paths.push(JSON.stringify(path));definitions.set(id,value);ids.set(value,id);
    }
    for(const key of Object.keys(value).sort())visit(value[key],[...path,key]);
  }
  visit(data,[]);
  return {paths,definitions,definitionId(def){const id=ids.get(def);if(!id)throw Error('combat definition absent from matched registry');return id;}};
}
