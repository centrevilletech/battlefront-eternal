// Detailed statistics are read facades. Shared accessors materialize their
// pending native record before a scalar is observed, including through a
// nested object retained by a scoreboard between publications.
const bindings = new WeakMap();
export function observedFields(names) {
  const descriptors = {};
  for (let index=0;index<names.length;index++) descriptors[names[index]] = {
    enumerable:true,configurable:true,
    get() {
      const binding=bindings.get(this),owner=binding.owner;
      if(owner.pending&&!owner.reading)owner.flush();
      return binding.values[index];
    },
    set(value) {
      const binding=bindings.get(this),owner=binding.owner;
      if(owner.pending&&!owner.reading)owner.flush();
      binding.values[index]=value;
    },
  };
  return {names,descriptors};
}
export function observeRecord(object, fields, owner) {
  let binding=bindings.get(object);
  if(!binding) {
    const values=new Array(fields.names.length);
    for(let i=0;i<values.length;i++)values[i]=object[fields.names[i]];
    binding={values,owner};bindings.set(object,binding);
    // Redefining existing own keys preserves field order and enumeration.
    // Descriptor functions are shared by the record type, not each instance.
    Object.defineProperties(object,fields.descriptors);
  } else binding.owner=owner;
}
