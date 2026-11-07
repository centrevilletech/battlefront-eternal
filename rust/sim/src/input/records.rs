//! InputMailbox's wire records keep the generator's layout. Only ownership
//! changes here: admitted banks survive queue consumption and refused pushes.
use crate::gen::*;

/// Reserve each bounded payload while admitting the connection. A paused
/// burst may fill every queue slot before any take can return a bank.
pub(super) fn reserved_entry()->Option<ImEntry> {
    let mut entry=ImEntry::default();
    entry.codes.try_reserve_exact(64).ok()?;
    entry.actions.try_reserve_exact(2048).ok()?;
    entry.look.try_reserve_exact(2048).ok()?;
    entry.view.try_reserve_exact(7).ok()?;
    entry.human.try_reserve_exact(16).ok()?;
    entry.fire.try_reserve_exact(16).ok()?;
    Some(entry)
}

/// Keep the admitted banks, rather than the temporary empty records of a
/// refused operation. Typed native callers can also supply their own entries.
pub(super) fn recycle(entries:&mut Vec<ImEntry>,mut entry:ImEntry) {
    entry.codes.clear();entry.actions.clear();entry.look.clear();
    entry.view.clear();entry.human.clear();entry.fire.clear();
    if entries.len()<entries.capacity() && entry.codes.capacity()>=64 && entry.actions.capacity()>=2048
        && entry.look.capacity()>=2048 && entry.view.capacity()>=7 && entry.human.capacity()>=16 && entry.fire.capacity()>=16 {entries.push(entry);}
}

/// InputMailboxCall's fixed fields never own variable storage. The caller has
/// checked the optional entry and each list's generated span before reading.
pub(super) fn call(row:&[f64])->ImInput {
    use InputMailboxCall as C;
    ImInput {op:ImOperation::from_u8(row[C::OP] as u8).unwrap_or_default(),
        sequence:row[C::SEQUENCE],epoch_matches:row[C::EPOCH_MATCHES]!=0.,
        tick:row[C::TICK],generation:row[C::GENERATION],..Default::default()}
}

/// ImEntry's generated list readers borrow the packet; fixed nested records
/// contain no vectors. No take may replace one of the reserved list banks.
pub(super) fn read_entry(entry:&mut ImEntry,row:&[f64],heap:&[f64]) {
    use ImEntry as E;
    entry.publication=WiView::take(&row[E::PUBLICATION..],heap);
    entry.identity=row[E::IDENTITY];entry.sequence=row[E::SEQUENCE];
    entry.tick=row[E::TICK];entry.generation=row[E::GENERATION];entry.down=row[E::DOWN];
    entry.look_total=(row[E::LOOK_TOTAL]==0.).then(||[row[E::LOOK_TOTAL+1],row[E::LOOK_TOTAL+2]]);
    entry.codes.extend(E::codes_rows(row,heap).map(|r|crate::store::StrId(crate::store::word_u32(r[0]))));
    entry.actions.extend(E::actions_rows(row,heap).map(|r|crate::store::StrId(crate::store::word_u32(r[0]))));
    entry.look.extend(E::look_rows(row,heap).map(|r|ImLook::take(r,heap)));
    entry.view.extend(E::view_rows(row,heap).map(|r|ImField::take(r,heap)));
    entry.human.extend(E::human_rows(row,heap).map(|r|HsCommand::take(r,heap)));
    entry.fire.extend(E::fire_rows(row,heap).map(|r|ImFire::take(r,heap)));
}
