import * as THREE from 'three';
import { createSanbornFrog } from '../src/frog.js';

// An illustrative lifecycle; this scene never calls inference or starts real workers.
export function mount() {
  const host = document.querySelector('.tadpole-pond');
  const canvas = document.querySelector('#tadpole-canvas');
  const motion = document.querySelector('#pond-motion');
  const summon = document.querySelector('#pond-summon');
  const status = document.querySelector('#pond-status');
  const payload = document.querySelector('#pond-payload');
  const payloadItems = [...document.querySelectorAll('.pond-key li>span:last-child')];
  const instructions = ['Read the README', 'Inspect the tests', 'Trace workspace paths'];
  const results = ['Context report + evidence', 'Coverage report + evidence', 'Boundary report + evidence'];
  const preference = matchMedia('(prefers-reduced-motion: reduce)');
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 1.5));
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.3;
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-5, 5, 3, -3, 0.1, 40);
  camera.position.set(0, 8, 11);
  camera.lookAt(0, 0, 0);
  scene.add(new THREE.HemisphereLight(0xfffbed, 0x4b6555, 3));
  const sun = new THREE.DirectionalLight(0xfff4d8, 3.5);
  sun.position.set(-3, 8, 5);
  scene.add(sun);
  const { frog } = createSanbornFrog();
  frog.position.set(2.7, 0, 0);
  frog.scale.setScalar(0.68);
  frog.rotation.y = -0.4;
  scene.add(frog);

  const padMaterial = new THREE.MeshStandardMaterial({ color: 0x39543a, roughness: 1, flatShading: true });
  const pad = new THREE.Mesh(new THREE.CylinderGeometry(1.45, 1.4, 0.12, 24), padMaterial);
  pad.position.set(2.7, -0.1, 0);
  scene.add(pad);
  const waterMaterial = new THREE.LineBasicMaterial({ color: 0x86a780, transparent: true, opacity: 0.23 });
  for (const radius of [1.75, 2.05, 2.4]) {
    const points = Array.from({ length: 100 }, (_, i) => {
      const angle = i / 100 * Math.PI * 2;
      return new THREE.Vector3(2.7 + Math.cos(angle) * radius, -0.13, Math.sin(angle) * radius * 0.8);
    });
    scene.add(new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(points), waterMaterial));
  }

  const bodyGeometry = new THREE.IcosahedronGeometry(0.3, 1);
  const eyeGeometry = new THREE.IcosahedronGeometry(0.052, 1);
  const eyeMaterial = new THREE.MeshStandardMaterial({ color: 0x172c23, roughness: 0.7 });
  const tailGeometry = new THREE.CylinderGeometry(0.085, 0.13, 0.36, 5);
  tailGeometry.rotateZ(Math.PI / 2);
  const colors = [0x8fd2c3, 0xbad18f, 0xe3c392];
  const workers = colors.map((color, index) => {
    const group = new THREE.Group();
    const material = new THREE.MeshStandardMaterial({ color, roughness: 0.8, flatShading: true });
    const body = new THREE.Mesh(bodyGeometry, material);
    body.scale.set(1.5, 0.85, 1);
    group.add(body);
    for (const side of [-1, 1]) {
      const eye = new THREE.Mesh(eyeGeometry, eyeMaterial);
      eye.position.set(0.22, 0.18, side * 0.19);
      group.add(eye);
    }
    const tails = [];
    for (let part = 0; part < 3; part++) {
      const joint = new THREE.Group();
      joint.position.x = part === 0 ? -0.32 : -0.36 * (1 - (part - 1) * 0.2);
      const segment = new THREE.Mesh(tailGeometry, material);
      segment.position.x = -0.18 * (1 - part * 0.2);
      segment.scale.setScalar(1 - part * 0.2);
      joint.add(segment);
      (part === 0 ? group : tails[part - 1]).add(joint);
      tails.push(joint);
    }
    const instruction = new THREE.Group();
    const paperMaterial = new THREE.MeshStandardMaterial({ color: 0xe3c392, roughness: 0.9 });
    const paper = new THREE.Mesh(new THREE.BoxGeometry(0.34, 0.025, 0.26), paperMaterial);
    instruction.add(paper);
    const rollGeometry = new THREE.CylinderGeometry(0.045, 0.045, 0.3, 8);
    rollGeometry.rotateX(Math.PI / 2);
    for (const x of [-0.16, 0.16]) {
      const roll = new THREE.Mesh(rollGeometry, paperMaterial);
      roll.position.x = x;
      instruction.add(roll);
    }
    instruction.position.set(0, 0.48, 0);
    group.add(instruction);
    const cargo = new THREE.Group();
    const resultMaterial = new THREE.MeshStandardMaterial({ color: 0x8fd2c3, roughness: 0.65 });
    const packet = new THREE.Mesh(new THREE.BoxGeometry(0.34, 0.06, 0.28), resultMaterial);
    cargo.add(packet);
    const tick = new THREE.Line(new THREE.BufferGeometry().setFromPoints([
      new THREE.Vector3(-0.1, 0.035, 0), new THREE.Vector3(-0.03, 0.035, 0.07),
      new THREE.Vector3(0.11, 0.035, -0.08),
    ]), new THREE.LineBasicMaterial({ color: 0x15251c }));
    cargo.add(tick);
    cargo.position.set(0, 0.48, 0);
    group.add(cargo);
    scene.add(group);
    const lane = (index - 1) * 1.8;
    const routePoints = Array.from({ length: 40 }, (_, i) => {
      const t = i / 39;
      return new THREE.Vector3(-3.6 + t * 5, -0.1, lane * (1 - t * 0.55));
    });
    const route = new THREE.Line(new THREE.BufferGeometry().setFromPoints(routePoints),
      new THREE.LineDashedMaterial({ color, dashSize: 0.08, gapSize: 0.15, transparent: true, opacity: 0.3 }));
    route.computeLineDistances();
    scene.add(route);
    return { group, tails, cargo, instruction, lane };
  });

  const stages = [
    ['Summon. Carry the instructions out.', 'Sanborn assigns the questions. Laya gates each dispatch.'],
    ['Investigate. Turn questions into evidence.', 'Laya checks read-only steps; instruction scrolls become result packets.'],
    ['Croak. Carry the results home.', 'Reports and evidence return to Sanborn, not acceptance certificates.'],
    ['Verify. Hand the results to Sanborn.', 'The parent evaluates each croak and verifies its findings.'],
  ];
  let paused = preference.matches;
  let visible = false;
  let failed = false;
  let elapsed = preference.matches ? 10 : 0;
  let previous = null;
  let frame = 0;
  let currentStage = -1;
  function draw(time) {
    frame = 0;
    if (failed) return;
    if (previous !== null && !paused) elapsed += Math.min((time - previous) / 1000, 0.05);
    previous = time;
    const cycle = elapsed % 16;
    const stage = cycle < 3 ? 0 : cycle < 6 ? 1 : cycle < 11 ? 2 : 3;
    if (stage !== currentStage) {
      currentStage = stage;
      status.querySelector('strong').textContent = stages[stage][0];
      status.querySelector('span').textContent = stages[stage][1];
      host.dataset.stage = String(stage);
      host.dataset.payload = stage < 2 ? 'instructions' : 'results';
      payload.textContent = stage < 2 ? 'OUTBOUND · Instruction scrolls' : 'INBOUND · Result packets';
      payloadItems.forEach((item, index) => {
        item.textContent = (stage < 2 ? instructions : results)[index];
      });
    }
    workers.forEach(({ group, tails, cargo, instruction, lane }, index) => {
      const outbound = Math.min(cycle / 3, 1);
      const inbound = THREE.MathUtils.clamp((cycle - 6 - index * 0.35) / 4, 0, 1);
      const distance = stage < 2 ? 1 - outbound : inbound;
      const x = -3.6 + distance * 4.65;
      const z = lane * (1 - distance * 0.55);
      group.position.set(x, 0.12 + Math.sin(elapsed * 3 + index) * 0.045, z);
      group.rotation.y = stage < 2 ? Math.PI : 0;
      group.rotation.z = Math.sin(elapsed * 4 + index) * 0.025;
      tails.forEach((joint, part) => {
        joint.rotation.y = Math.sin(elapsed * 8 - part * 0.85 + index) * 0.25;
      });
      cargo.visible = stage >= 2;
      instruction.visible = stage < 2;
    });
    renderer.render(scene, camera);
    if (!paused && visible && !document.hidden) frame = requestAnimationFrame(draw);
  }
  function refresh() {
    cancelAnimationFrame(frame);
    previous = null;
    if (visible && !document.hidden && !failed) frame = requestAnimationFrame(draw);
  }
  function controls() {
    motion.textContent = paused ? 'Resume swimming' : 'Pause swimming';
    motion.setAttribute('aria-pressed', String(paused));
    refresh();
  }
  function toggle() { paused = !paused; controls(); }
  function replay() {
    elapsed = preference.matches ? 10 : 0;
    paused = preference.matches;
    controls();
  }
  function changePreference() { paused = preference.matches; controls(); }
  function contextLost(event) {
    event.preventDefault();
    failed = true;
    cancelAnimationFrame(frame);
    host.classList.remove('loaded');
    motion.hidden = true;
    summon.hidden = true;
    document.querySelector('#pond-error').hidden = false;
    console.error('Tadpole pond: WebGL context lost.');
  }
  motion.hidden = false;
  summon.hidden = false;
  motion.addEventListener('click', toggle);
  summon.addEventListener('click', replay);
  preference.addEventListener('change', changePreference);
  canvas.addEventListener('webglcontextlost', contextLost);
  document.addEventListener('visibilitychange', refresh);
  const resize = new ResizeObserver(() => {
    if (!host.clientWidth || !host.clientHeight) return;
    renderer.setSize(host.clientWidth, host.clientHeight, false);
    const aspect = host.clientWidth / host.clientHeight;
    // Fit the whole pond on narrow screens rather than cropping its workers.
    const halfWidth = Math.max(5, 3 * aspect);
    camera.left = -halfWidth;
    camera.right = halfWidth;
    camera.top = halfWidth / aspect;
    camera.bottom = -halfWidth / aspect;
    camera.updateProjectionMatrix();
    refresh();
  });
  resize.observe(host);
  const observer = new IntersectionObserver(entries => {
    visible = entries[0].isIntersecting;
    refresh();
  });
  observer.observe(host);
  function dispose(event) {
    if (event.persisted) return;
    failed = true;
    cancelAnimationFrame(frame);
    resize.disconnect();
    observer.disconnect();
    motion.removeEventListener('click', toggle);
    summon.removeEventListener('click', replay);
    preference.removeEventListener('change', changePreference);
    document.removeEventListener('visibilitychange', refresh);
    canvas.removeEventListener('webglcontextlost', contextLost);
    const geometries = new Set();
    const materials = new Set();
    scene.traverse(object => {
      if (object.geometry) geometries.add(object.geometry);
      if (object.material) materials.add(object.material);
    });
    geometries.forEach(geometry => geometry.dispose());
    materials.forEach(material => material.dispose());
    renderer.dispose();
    removeEventListener('pagehide', dispose);
  }
  addEventListener('pagehide', dispose);
  host.classList.add('loaded');
  controls();
}
