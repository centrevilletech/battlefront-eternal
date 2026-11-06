// A page-local observer. Workers and ordinary play leave it unset. Keeping
// the hook here lets animation and timeline sampling retain their ownership.
export let presentationProfile = null;
export function observePresentation(profile) { presentationProfile = profile; }
