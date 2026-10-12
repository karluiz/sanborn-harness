import * as THREE from 'three';
import { createSanbornFrog } from '../src/frog.js';

// An illustrative lifecycle; this scene never calls inference or starts real workers.
export function mount() {
  const host = document.querySelector('.tadpole-pond');
  const canvas = document.querySelector('#tadpole-canvas');
  const motion = document.querySelector('#pond-motion');
  const summon = document.querySelector('#pond-summon');
  const sound = document.querySelector('#pond-sound');
  const status = document.querySelector('#pond-status');
  const arrivalStatus = document.querySelector('#pond-arrival');
  const croarCue = document.querySelector('#pond-croar');
  const payload = document.querySelector('#pond-payload');
  const payloadItems = [...document.querySelectorAll('.pond-key li>span:last-child')];
  const instructions = ['Read the README', 'Inspect the tests', 'Trace workspace paths'];
  const results = ['Context report + evidence', 'Coverage report + evidence', 'Boundary report + evidence'];
  const schedules = [
    { departure: 0, travel: 2.5, returnAt: 6, returnTravel: 2.8 },
    { departure: 0.6, travel: 2.8, returnAt: 8.5, returnTravel: 3.2 },
    { departure: 1.2, travel: 3, returnAt: 11, returnTravel: 3.8 },
  ];
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
  const arrivalRings = [0xf3e5a7, 0x8fd2c3, 0xbad18f].map(color => {
    const ring = new THREE.Mesh(
      new THREE.TorusGeometry(0.2, 0.018, 6, 40),
      new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0 }),
    );
    ring.rotation.x = Math.PI / 2;
    ring.position.set(2.7, -0.025, 0);
    scene.add(ring);
    return ring;
  });

  const bodyGeometry = new THREE.IcosahedronGeometry(1, 2);
  const darkMaterial = new THREE.MeshStandardMaterial({ color: 0x233025, roughness: 0.55 });
  const creamMaterial = new THREE.MeshStandardMaterial({ color: 0xf2efcf, roughness: 0.85, flatShading: true });
  const whiteMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff });
  const inkMaterial = new THREE.MeshStandardMaterial({ color: 0x3c5346, roughness: 0.9 });
  function pebble(parent, material, position, scale) {
    const mesh = new THREE.Mesh(bodyGeometry, material);
    mesh.position.set(...position);
    mesh.scale.set(...scale);
    parent.add(mesh);
    return mesh;
  }
  function box(parent, material, position, size) {
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(...size), material);
    mesh.position.set(...position);
    parent.add(mesh);
    return mesh;
  }
  // One tapered skin, deformed in place: no disconnected tail segments or frame allocations.
  function createTail(material) {
    const length = 18;
    const sides = 8;
    const positions = new Float32Array((length + 1) * sides * 3);
    const indices = [];
    for (let ring = 0; ring < length; ring++) {
      for (let side = 0; side < sides; side++) {
        const a = ring * sides + side;
        const b = ring * sides + (side + 1) % sides;
        indices.push(a, a + sides, b, b, a + sides, b + sides);
      }
    }
    const geometry = new THREE.BufferGeometry();
    const attribute = new THREE.BufferAttribute(positions, 3);
    attribute.setUsage(THREE.DynamicDrawUsage);
    geometry.setAttribute('position', attribute);
    geometry.setIndex(indices);
    const mesh = new THREE.Mesh(geometry, material);
    // The animated skin stays within the worker's known bounds.
    geometry.boundingSphere = new THREE.Sphere(new THREE.Vector3(-1, 0, 0), 1.1);
    return {
      mesh,
      update(time, strength) {
        for (let ring = 0; ring <= length; ring++) {
          const t = ring / length;
          const radius = 0.16 * (1 - t) ** 1.3 + 0.008;
          const bend = Math.sin(time * 7 - t * 4.5) * t * t * 0.3 * strength;
          for (let side = 0; side < sides; side++) {
            const angle = side / sides * Math.PI * 2;
            const offset = (ring * sides + side) * 3;
            positions[offset] = -0.32 - t * 1.1;
            positions[offset + 1] = Math.sin(angle) * radius * 0.55;
            positions[offset + 2] = bend + Math.cos(angle) * radius;
          }
        }
        attribute.needsUpdate = true;
        geometry.computeVertexNormals();
      },
    };
  }
  const colors = [0x8fd2c3, 0xbad18f, 0xe3c392];
  const workers = colors.map((color, index) => {
    const group = new THREE.Group();
    const material = new THREE.MeshStandardMaterial({ color, roughness: 0.8, flatShading: true });
    pebble(group, material, [0, 0, 0], [0.52, 0.32, 0.38]);
    pebble(group, creamMaterial, [0.14, -0.11, 0], [0.35, 0.23, 0.31]);
    for (const side of [-1, 1]) {
      pebble(group, material, [0.2, 0.24, side * 0.22], [0.19, 0.18, 0.16]);
      pebble(group, creamMaterial, [0.27, 0.26, side * 0.29], [0.13, 0.13, 0.085]);
      pebble(group, darkMaterial, [0.31, 0.27, side * 0.355], [0.068, 0.084, 0.025]);
      pebble(group, whiteMaterial, [0.33, 0.307, side * 0.374], [0.023, 0.023, 0.01]);
    }
    const smile = new THREE.CatmullRomCurve3([
      new THREE.Vector3(0.45, 0.025, -0.13),
      new THREE.Vector3(0.5, -0.015, 0),
      new THREE.Vector3(0.45, 0.025, 0.13),
    ]);
    group.add(new THREE.Mesh(new THREE.TubeGeometry(smile, 10, 0.009, 5, false), darkMaterial));
    const tail = createTail(material);
    group.add(tail.mesh);
    const instruction = new THREE.Group();
    const paperMaterial = new THREE.MeshStandardMaterial({ color: 0xe3c392, roughness: 0.9 });
    box(instruction, paperMaterial, [0, 0, 0], [0.48, 0.025, 0.48]);
    const rollGeometry = new THREE.CylinderGeometry(0.065, 0.065, 0.52, 12);
    rollGeometry.rotateX(Math.PI / 2);
    for (const x of [-0.24, 0.24]) {
      const roll = new THREE.Mesh(rollGeometry, paperMaterial);
      roll.position.x = x;
      instruction.add(roll);
    }
    for (let row = 0; row < 3; row++) {
      box(instruction, inkMaterial, [-0.015, 0.018, -0.12 + row * 0.09], [0.24 - row * 0.04, 0.008, 0.015]);
    }
    const seal = new THREE.Mesh(new THREE.CylinderGeometry(0.052, 0.052, 0.016, 8),
      new THREE.MeshStandardMaterial({ color: 0x985b40, roughness: 0.9 }));
    seal.position.set(0.13, 0.027, 0.13);
    instruction.add(seal);
    instruction.position.set(-0.12, 0.63, 0);
    instruction.rotation.z = 0.2;
    group.add(instruction);
    const cargo = new THREE.Group();
    const resultMaterial = new THREE.MeshStandardMaterial({ color: 0x8fd2c3, roughness: 0.65 });
    box(cargo, resultMaterial, [0, 0, 0], [0.52, 0.04, 0.57]);
    box(cargo, resultMaterial, [-0.14, 0, -0.3], [0.2, 0.04, 0.08]);
    for (let sheet = 0; sheet < 3; sheet++) {
      const paper = box(cargo, creamMaterial, [sheet * 0.015, 0.034 + sheet * 0.019, -sheet * 0.014], [0.43, 0.015, 0.48]);
      paper.rotation.y = sheet * 0.025;
    }
    for (let row = 0; row < 3; row++) {
      box(cargo, inkMaterial, [0.04, 0.086, -0.1 + row * 0.09], [0.26 - row * 0.04, 0.008, 0.015]);
    }
    box(cargo, resultMaterial, [-0.145, 0.088, -0.1], [0.044, 0.012, 0.044]);
    cargo.position.set(-0.12, 0.63, 0);
    cargo.rotation.z = 0.2;
    group.add(cargo);
    scene.add(group);
    const lane = (index - 1) * 2;
    const path = new THREE.CubicBezierCurve3(
      new THREE.Vector3(0.95, 0, lane * 0.48),
      new THREE.Vector3(-0.5, 0, lane * 0.9 - 0.3),
      new THREE.Vector3(-2.25, 0, lane + 0.5),
      new THREE.Vector3(-3.1, 0, lane),
    );
    const routePoints = path.getPoints(40).map(point => point.setY(-0.12));
    const route = new THREE.Line(new THREE.BufferGeometry().setFromPoints(routePoints),
      new THREE.LineDashedMaterial({ color, dashSize: 0.05, gapSize: 0.14, transparent: true, opacity: 0.18 }));
    route.computeLineDistances();
    scene.add(route);
    const wake = new THREE.Group();
    const wakeMaterial = new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.25 });
    for (let ripple = 0; ripple < 3; ripple++) {
      const points = Array.from({ length: 18 }, (_, i) => {
        const a = -0.8 + i / 17 * 1.6;
        return new THREE.Vector3(-0.5 - ripple * 0.35 - Math.cos(a) * 0.22, 0,
          Math.sin(a) * (0.32 + ripple * 0.15));
      });
      wake.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), wakeMaterial));
    }
    scene.add(wake);
    return { group, tail, cargo, instruction, lane, path, wake, wakeMaterial };
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
  let audioContext = null;
  let muted = false;
  let hasDrawn = false;
  let lastWorkerStates = [];
  let lastCroakAt = -Infinity;
  const ringStartedAt = Array(3).fill(-Infinity);
  let croarTimer = 0;
  const audioAvailable = Boolean(window.AudioContext || window.webkitAudioContext);
  const workerNames = ['README', 'Tests', 'Paths'];
  function updateSoundControl() {
    sound.textContent = muted ? 'Croaks off' : 'Croaks on';
    sound.setAttribute('aria-pressed', String(!muted));
  }
  function unlockAudio() {
    if (!audioAvailable) {
      arrivalStatus.textContent = 'Audio is not available in this browser; the visual croar cue still plays.';
      return;
    }
    if (!audioContext) {
      const AudioContextClass = window.AudioContext || window.webkitAudioContext;
      audioContext = new AudioContextClass();
    }
    if (audioContext.state !== 'running' && audioContext.state !== 'closed') {
      audioContext.resume().catch(() => {
        muted = true;
        updateSoundControl();
        arrivalStatus.textContent = 'Browser audio could not start; use Enable croaks to try again.';
      });
    }
  }
  function playCroak() {
    if (muted || !audioContext || audioContext.state !== 'running') return;
    const now = audioContext.currentTime;
    const oscillator = audioContext.createOscillator();
    const filter = audioContext.createBiquadFilter();
    const volume = audioContext.createGain();
    oscillator.type = 'sawtooth';
    oscillator.frequency.setValueAtTime(155, now);
    oscillator.frequency.exponentialRampToValueAtTime(78, now + 0.24);
    filter.type = 'bandpass';
    filter.frequency.setValueAtTime(390, now);
    filter.frequency.exponentialRampToValueAtTime(210, now + 0.28);
    filter.Q.value = 3.5;
    volume.gain.setValueAtTime(0.0001, now);
    volume.gain.exponentialRampToValueAtTime(0.19, now + 0.025);
    volume.gain.exponentialRampToValueAtTime(0.075, now + 0.12);
    volume.gain.exponentialRampToValueAtTime(0.15, now + 0.17);
    volume.gain.exponentialRampToValueAtTime(0.0001, now + 0.39);
    oscillator.connect(filter).connect(volume).connect(audioContext.destination);
    oscillator.start(now);
    oscillator.stop(now + 0.41);
    oscillator.onended = () => {
      oscillator.disconnect();
      filter.disconnect();
      volume.disconnect();
    };
  }
  function celebrateArrival(index) {
    lastCroakAt = elapsed;
    ringStartedAt[index] = elapsed;
    playCroak();
    arrivalStatus.textContent = `${workerNames[index]} tadpole arrived. Croar! Sanborn received its illustrative evidence; the parent still verifies.`;
    croarCue.classList.remove('is-visible');
    clearTimeout(croarTimer);
    requestAnimationFrame(() => croarCue.classList.add('is-visible'));
    croarTimer = window.setTimeout(() => croarCue.classList.remove('is-visible'), 1800);
  }
  function draw(time) {
    frame = 0;
    if (failed) return;
    if (previous !== null && !paused) elapsed += Math.min((time - previous) / 1000, 0.05);
    previous = time;
    const cycle = elapsed % 20;
    const stage = cycle < 3 ? 0 : cycle < 6 ? 1 : cycle < 15 ? 2 : 3;
    if (stage !== currentStage) {
      currentStage = stage;
      status.querySelector('strong').textContent = stages[stage][0];
      status.querySelector('span').textContent = stages[stage][1];
      host.dataset.stage = String(stage);
      host.dataset.payload = stage < 2 ? 'instructions' : stage === 2 ? 'mixed' : 'results';
      payload.textContent = stage < 2 ? 'OUTBOUND · Instruction scrolls'
        : stage === 2 ? 'ASYNC · Results return independently' : 'RECEIVED · Three separate croaks';
    }
    const workerStates = [];
    workers.forEach(({ group, tail, cargo, instruction, lane, path, wake, wakeMaterial }, index) => {
      const { departure, travel, returnAt, returnTravel } = schedules[index];
      const arrivedAt = departure + travel;
      const dockedAt = returnAt + returnTravel;
      const returning = cycle >= returnAt;
      const investigating = cycle >= arrivedAt && !returning;
      const docked = cycle >= dockedAt;
      const state = docked ? 'croaked' : returning ? 'returning' : investigating ? 'investigating'
        : cycle < departure ? 'assigned' : 'outbound';
      if (hasDrawn && state === 'croaked' && lastWorkerStates[index] !== 'croaked') {
        celebrateArrival(index);
      }
      lastWorkerStates[index] = state;
      workerStates.push(state);
      const text = `${state === 'croaked' ? 'Croak received' : state === 'returning' ? 'Returning' :
        state === 'investigating' ? 'Investigating' : state === 'assigned' ? 'Assigned' : 'Dispatched'} · ${
        returning ? results[index] : instructions[index]}`;
      if (payloadItems[index].textContent !== text) payloadItems[index].textContent = text;
      const outbound = THREE.MathUtils.clamp((cycle - departure) / travel, 0, 1);
      const inbound = THREE.MathUtils.clamp((cycle - returnAt) / returnTravel, 0, 1);
      const smooth = t => t * t * (3 - 2 * t);
      const t = returning ? 1 - smooth(inbound) : smooth(outbound);
      const position = path.getPoint(t);
      const tangent = path.getTangent(t);
      let heading = Math.atan2(-tangent.z, tangent.x);
      if (investigating) {
        const turn = smooth((cycle - arrivedAt) / (returnAt - arrivedAt));
        heading -= turn * Math.PI;
        position.x -= Math.sin(turn * Math.PI) * 0.25;
        position.z += Math.sin(turn * Math.PI) * 0.35;
      } else if (returning) {
        heading -= Math.PI;
        if (docked) heading -= smooth((cycle - dockedAt) / (20 - dockedAt)) * Math.PI;
      }
      group.position.copy(position);
      group.position.y = 0.17 + Math.sin(elapsed * 2.4 + index) * 0.035;
      group.rotation.y = heading;
      group.rotation.z = Math.sin(elapsed * 4 + index) * 0.035;
      const swimming = state === 'outbound' || state === 'returning';
      tail.update(elapsed + index, swimming ? 1 : 0.35);
      wake.position.set(position.x, -0.07, position.z);
      wake.rotation.y = heading;
      wakeMaterial.opacity = swimming ? 0.22 : 0.06;
      cargo.visible = returning;
      instruction.visible = !returning;
      const exchange = investigating ? Math.sin((cycle - arrivedAt) / (returnAt - arrivedAt) * Math.PI) : 0;
      instruction.position.y = 0.63 + exchange * 0.1;
      cargo.position.y = 0.63 + Math.sin(elapsed * 2 + index) * 0.025;
    });
    hasDrawn = true;
    host.dataset.workers = workerStates.join(',');
    const croakAge = elapsed - lastCroakAt;
    const croakMotion = croakAge >= 0 && croakAge < 0.85
      ? Math.sin(Math.min(croakAge / 0.85, 1) * Math.PI)
      : 0;
    frog.position.y = croakMotion * 0.075;
    frog.scale.setScalar(0.68 * (1 + croakMotion * 0.035));
    arrivalRings.forEach((ring, index) => {
      const age = elapsed - ringStartedAt[index];
      const active = age >= 0 && age < 1.1;
      const progress = active ? age / 1.1 : 0;
      ring.scale.setScalar(active ? 0.45 + progress * 3.1 : 0.45);
      ring.material.opacity = active ? (1 - progress) * 0.56 : 0;
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
    if (!muted) unlockAudio();
    controls();
  }
  function toggleSound() {
    muted = !muted;
    updateSoundControl();
    if (!muted) unlockAudio();
  }
  function changePreference() { paused = preference.matches; controls(); }
  function contextLost(event) {
    event.preventDefault();
    failed = true;
    cancelAnimationFrame(frame);
    host.classList.remove('loaded');
    motion.hidden = true;
    summon.hidden = true;
    sound.hidden = true;
    document.querySelector('#pond-error').hidden = false;
    console.error('Tadpole pond: WebGL context lost.');
  }
  motion.hidden = false;
  summon.hidden = false;
  sound.hidden = !audioAvailable;
  updateSoundControl();
  motion.addEventListener('click', toggle);
  summon.addEventListener('click', replay);
  sound.addEventListener('click', toggleSound);
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
    sound.removeEventListener('click', toggleSound);
    preference.removeEventListener('change', changePreference);
    document.removeEventListener('visibilitychange', refresh);
    canvas.removeEventListener('webglcontextlost', contextLost);
    clearTimeout(croarTimer);
    if (audioContext && audioContext.state !== 'closed') void audioContext.close();
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
