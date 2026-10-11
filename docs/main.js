const nodes = {
  client: ['CLI / client', 'sb -p supplies the task and workspace. The user’s objective stays authoritative; a generated plan never replaces it.', 'The first CLI slice supports file tools, not arbitrary shell execution.'],
  runtime: ['Rust runtime', 'Owns state, budgets and cancellation. Continues validated plan steps locally; asks for reasoning when the evidence or plan no longer holds.', 'Completing a plan is not proof of task success.'],
  policy: ['Permission gate', 'Checks every ready step against the run policy. A recommendation to continue does not change what the run is allowed to do.', 'Policy flags and path checks are not an operating-system sandbox.'],
  scheduler: ['Tool scheduler', 'Bounds concurrent independent reads and orders mutations. A step becomes ready only when its explicit dependencies are satisfied.', 'Dependent operations cannot be made safe by calling them parallel.'],
  state: ['Results & state', 'Records outputs, errors and acceptance evidence. The proposed cache reuses results only while their validity is known.', 'Edits invalidate affected reads. Shell commands conservatively invalidate workspace-derived results.'],
  laya: ['Laya decision layer', 'A required part of the planned runner. Returns Continue, Replan or explicit Absent. The first local evaluator checks exact evidence guards; learned backend embedding comes later.', 'Advice never grants permissions, invents edits or certifies success.'],
  reasoning: ['Reasoning adapter', 'Calls a model for missing plans, ambiguity or unexpected evidence. Its returned plan must be validated before execution.', 'An avoided call is an optimization, not permission to skip necessary reasoning.'],
  servers: ['Inference servers', 'Azure Chat Completions is connected for reasoning. Other deployments and any learned decision backend need individual protocol and capability checks.', 'Only approved hosts and data classes cross this boundary. Start with synthetic fixtures.'],
};
const routes = {
  all: Object.keys(nodes),
  fast: ['client', 'runtime', 'policy', 'scheduler', 'state', 'laya'],
  reason: ['client', 'runtime', 'reasoning', 'servers'],
  absent: ['state', 'laya', 'reasoning', 'servers'],
};
const routeText = {
  all: 'Select a component to inspect its responsibilities.',
  fast: 'Laya evaluates each transition. Execute a ready planned step through the permission gate, then check its evidence. Rust still enforces preconditions.',
  reason: 'A missing or invalid plan calls for reasoning through an approved backend. Validate the resulting plan before execution.',
  absent: 'An unavailable or uncertain Laya decision returns to the normal reasoning loop with its reason recorded.',
};
document.querySelectorAll('[data-node]').forEach(button => button.addEventListener('click', () => {
  const [title, copy, limit] = nodes[button.dataset.node];
  document.querySelectorAll('[data-node]').forEach(node => node.setAttribute('aria-pressed', String(node === button)));
  document.querySelector('#node-title').textContent = title;
  document.querySelector('#node-copy').textContent = copy;
  document.querySelector('#node-limit').textContent = `Boundary: ${limit}`;
}));
document.querySelectorAll('[data-route]').forEach(button => button.addEventListener('click', () => {
  const route = button.dataset.route;
  document.querySelectorAll('[data-route]').forEach(option => option.setAttribute('aria-pressed', String(option === button)));
  document.querySelectorAll('[data-node]').forEach(node => node.classList.toggle('dim', !routes[route].includes(node.dataset.node)));
  document.querySelectorAll('[data-wire]').forEach(wire => wire.classList.toggle('dim', route !== 'all' && !wire.dataset.wire.split(' ').includes(route)));
  document.querySelector('#route-status').textContent = routeText[route];
}));
const chapters = [...document.querySelectorAll('.chapter')];
const links = [...document.querySelectorAll('.contents nav a')];
let scheduled = false;
function updateChapter() {
  const current = [...chapters].reverse().find(section => section.getBoundingClientRect().top <= innerHeight * 0.35) || chapters[0];
  links.forEach(link => {
    if (link.hash === `#${current.id}`) link.setAttribute('aria-current', 'location');
    else link.removeAttribute('aria-current');
  });
  scheduled = false;
}
addEventListener('scroll', () => {
  if (!scheduled) { scheduled = true; requestAnimationFrame(updateChapter); }
}, { passive: true });
updateChapter();
import('./scene.js').then(module => module.mount()).catch(error => {
  console.error('Sanborn sculpture could not load:', error);
  document.querySelector('.specimen').classList.remove('loaded');
  document.querySelector('#motion').hidden = true;
  document.querySelector('#scene-error').hidden = false;
});
