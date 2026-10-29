// Keep the latest main-branch installer for each platform. Release assets live
// separately and are never touched; older installers can be rebuilt from Git.
const { GITHUB_TOKEN: token, GITHUB_REPOSITORY: repository, ARTIFACT_NAME: name } = process.env;
if (!token || !repository || !/^battlefront-eternal-(linux|macos|windows)$/.test(name ?? '')) {
  throw new Error('Missing artifact cleanup configuration');
}
async function api(path, method = 'GET') {
  const response = await fetch(`https://api.github.com/repos/${repository}/${path}`, {
    method,
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
  });
  if (!response.ok) throw new Error(`Artifact API ${method} failed: ${response.status}`);
  return response.status === 204 ? null : response.json();
}
const artifacts = [];
for (let page = 1; ; page++) {
  const result = await api(`actions/artifacts?name=${encodeURIComponent(name)}&per_page=100&page=${page}`);
  artifacts.push(...result.artifacts);
  if (result.artifacts.length < 100) break;
}
const installers = artifacts.filter(artifact => artifact.name === name && !artifact.expired && artifact.workflow_run?.head_branch === 'main')
  .sort((a, b) => b.created_at.localeCompare(a.created_at) || b.id - a.id);
for (const artifact of installers.slice(1)) {
  await api(`actions/artifacts/${artifact.id}`, 'DELETE');
  console.log(`Removed superseded ${name} artifact ${artifact.id}`);
}
