import * as THREE from 'three';

const canvas = document.querySelector('#frog-scene');
const motionButton = document.querySelector('#motion-toggle');
const hopButton = document.querySelector('#hop-button');
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const dialog = document.querySelector('#about-dialog');
document.querySelector('#about-open').addEventListener('click', () => dialog.showModal());
document.querySelector('#about-close').addEventListener('click', () => dialog.close());
dialog.addEventListener('click', (event) => {
  const rect = dialog.getBoundingClientRect();
  if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close();
});

function showSceneError(error) {
  console.error('Unable to render Freddy’s pond:', error);
  document.querySelector('#scene-error').hidden = false;
  document.querySelectorAll('.controls button').forEach((button) => { button.disabled = true; });
}

function createPond() {
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.35;

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(34, 1, 0.1, 80);
  const world = new THREE.Group();
  scene.add(world);
  scene.add(new THREE.HemisphereLight(0xfffbed, 0x6c7958, 2.5));
  const sun = new THREE.DirectionalLight(0xfff5dc, 4);
  sun.position.set(-3, 8, 5);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  sun.shadow.camera.left = -7;
  sun.shadow.camera.right = 7;
  sun.shadow.camera.top = 7;
  sun.shadow.camera.bottom = -7;
  sun.shadow.normalBias = 0.03;
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0xcddfc7, 1.1);
  fill.position.set(6, 4, -3);
  scene.add(fill);

  const frog = new THREE.Group();
  world.add(frog);
  const green = new THREE.MeshStandardMaterial({ color: 0x6f9252, roughness: 0.88, flatShading: true });
  const lightGreen = new THREE.MeshStandardMaterial({ color: 0x8da969, roughness: 0.9, flatShading: true });
  const bellyMaterial = new THREE.MeshStandardMaterial({ color: 0xc6d39b, roughness: 1, flatShading: true });
  const eyeMaterial = new THREE.MeshStandardMaterial({ color: 0xf2efcf, roughness: 0.5, flatShading: true });
  const darkMaterial = new THREE.MeshStandardMaterial({ color: 0x233025, roughness: 0.45 });
  const highlightMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff });

  function mesh(geometry, material, position, scale, parent = frog) {
    const object = new THREE.Mesh(geometry, material);
    object.position.set(...position);
    if (scale) object.scale.set(...scale);
    object.castShadow = true;
    object.receiveShadow = true;
    parent.add(object);
    return object;
  }
  function pebble(material, position, scale, detail = 1, parent = frog) {
    return mesh(new THREE.IcosahedronGeometry(1, detail), material, position, scale, parent);
  }
  function limb(start, end, radius, material = green) {
    const a = new THREE.Vector3(...start);
    const b = new THREE.Vector3(...end);
    const cylinder = mesh(new THREE.CylinderGeometry(radius * 0.8, radius, a.distanceTo(b), 5), material, a.clone().add(b).multiplyScalar(0.5).toArray());
    cylinder.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), b.sub(a).normalize());
    return cylinder;
  }

  pebble(green, [0, 1.02, -0.13], [1.1, 0.95, 0.93]);
  pebble(bellyMaterial, [0, 0.9, 0.53], [0.82, 0.76, 0.47]);
  pebble(green, [0, 1.75, 0.34], [1.2, 0.67, 0.87]);
  pebble(lightGreen, [0, 1.47, 0.78], [0.97, 0.31, 0.49]);
  for (const side of [-1, 1]) {
    pebble(green, [side * 0.75, 2.12, 0.43], [0.48, 0.49, 0.47]);
    pebble(eyeMaterial, [side * 0.75, 2.15, 0.73], [0.32, 0.34, 0.2], 2);
    pebble(darkMaterial, [side * 0.73, 2.16, 0.905], [0.14, 0.2, 0.065], 2);
    pebble(highlightMaterial, [side * 0.73 - 0.045, 2.23, 0.957], [0.046, 0.056, 0.021], 1);
    pebble(green, [side * 1.12, 0.56, -0.32], [0.65, 0.61, 0.79]);
    limb([side * 1.32, 0.5, -0.31], [side * 1.45, 0.18, 0.54], 0.22);
    pebble(lightGreen, [side * 1.43, 0.15, 0.62], [0.42, 0.15, 0.44]);
    limb([side * 0.69, 1.19, 0.56], [side * 0.8, 0.6, 0.84], 0.16);
    limb([side * 0.8, 0.6, 0.84], [side * 0.84, 0.16, 1.14], 0.12, lightGreen);
    pebble(lightGreen, [side * 0.85, 0.13, 1.2], [0.25, 0.12, 0.26]);
    for (let toe = 0; toe < 3; toe++) {
      const x = side * (0.68 + toe * 0.17);
      limb([side * 0.83, 0.11, 1.19], [x, 0.08, 1.52 + (toe === 1 ? 0.07 : 0)], 0.045, lightGreen);
      pebble(lightGreen, [x, 0.08, 1.52 + (toe === 1 ? 0.07 : 0)], [0.078, 0.045, 0.095], 0);
      limb([side * 1.43, 0.12, 0.75], [side * (1.2 + toe * 0.23), 0.075, 1.04], 0.06, lightGreen);
    }
    pebble(darkMaterial, [side * 0.3, 1.74, 1.127], [0.037, 0.025, 0.02], 1);
  }
  const smilePoints = [];
  for (let i = 0; i <= 20; i++) {
    const x = (i / 20 - 0.5) * 1.25;
    smilePoints.push(new THREE.Vector3(x, 1.43 + 0.15 * (x / 0.625) ** 2, 1.12 - 0.15 * (x / 0.625) ** 2));
  }
  mesh(new THREE.TubeGeometry(new THREE.CatmullRomCurve3(smilePoints), 20, 0.017, 5, false), darkMaterial, [0, 0, 0]);

  const padMaterial = new THREE.MeshStandardMaterial({ color: 0x718956, roughness: 1, flatShading: true, side: THREE.DoubleSide });
  const padShape = new THREE.Shape();
  const padRadius = 2.55;
  padShape.moveTo(0.08, -0.25);
  for (let i = 0; i <= 28; i++) {
    const angle = -Math.PI / 2 + 0.21 + (i / 28) * (Math.PI * 2 - 0.42);
    padShape.lineTo(Math.cos(angle) * padRadius, Math.sin(angle) * padRadius * 0.91);
  }
  padShape.lineTo(0.08, -0.25);
  const pad = mesh(new THREE.ExtrudeGeometry(padShape, { depth: 0.085, bevelEnabled: false }), padMaterial, [0, -0.05, 0], null, world);
  pad.rotation.x = -Math.PI / 2;
  const veinMaterial = new THREE.LineBasicMaterial({ color: 0x9aaf79, transparent: true, opacity: 0.35 });
  for (let i = 0; i < 11; i++) {
    const angle = 0.15 + i * Math.PI * 2 / 11;
    const points = [new THREE.Vector3(0, 0.04, 0), new THREE.Vector3(Math.cos(angle) * 2.4, 0.04, Math.sin(angle) * 2.15)];
    world.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), veinMaterial));
  }
  const waterMaterial = new THREE.MeshStandardMaterial({ color: 0xbfcdb4, roughness: 0.68, transparent: true, opacity: 0.6 });
  mesh(new THREE.CylinderGeometry(3.35, 3.25, 0.12, 64), waterMaterial, [0, -0.22, 0], null, world);
  const ringMaterial = new THREE.MeshBasicMaterial({ color: 0xa5b698, transparent: true, opacity: 0.35, side: THREE.DoubleSide });
  for (const radius of [3.65, 4.02, 4.55]) {
    const ring = mesh(new THREE.RingGeometry(radius, radius + 0.014, 100), ringMaterial, [0, -0.25, 0], null, world);
    ring.rotation.x = -Math.PI / 2;
  }
  const ground = mesh(new THREE.PlaneGeometry(200, 200), new THREE.ShadowMaterial({ opacity: 0.13 }), [0, -0.3, 0], null, scene);
  ground.rotation.x = -Math.PI / 2;
  ground.castShadow = false;
  const rockMaterial = new THREE.MeshStandardMaterial({ color: 0xb9bda5, flatShading: true, roughness: 1 });
  pebble(rockMaterial, [-2.95, -0.13, 1.9], [0.31, 0.18, 0.26], 0, world);
  pebble(rockMaterial, [-3.22, -0.18, 1.55], [0.19, 0.11, 0.16], 0, world);
  const flower = new THREE.Group();
  flower.position.set(2.15, 0.02, -1.25);
  world.add(flower);
  const petalMaterial = new THREE.MeshStandardMaterial({ color: 0xead9c5, roughness: 1, flatShading: true });
  for (let i = 0; i < 6; i++) {
    const angle = i * Math.PI / 3;
    const petal = pebble(petalMaterial, [Math.cos(angle) * 0.19, 0.11, Math.sin(angle) * 0.19], [0.17, 0.09, 0.1], 0, flower);
    petal.rotation.y = -angle;
  }
  pebble(new THREE.MeshStandardMaterial({ color: 0xcab576, flatShading: true }), [0, 0.15, 0], [0.12, 0.08, 0.12], 0, flower);

  let yaw = -0.32;
  let targetYaw = yaw;
  let paused = reducedMotion.matches;
  let dragging = false;
  let lastX = 0;
  let hopStart = null;
  let disposed = false;
  let frame;
  let elapsed = 0;
  let lastTime = performance.now();
  const palettes = {
    green: [0x6f9252, 0x8da969, 0xc6d39b],
    clay: [0xb77a5a, 0xca9676, 0xe2c5a3],
    blue: [0x6a8f9c, 0x8eacb4, 0xc8d8ce],
  };

  function updateMotionButton() {
    motionButton.setAttribute('aria-pressed', String(paused));
    motionButton.setAttribute('aria-label', paused ? 'Resume animation' : 'Pause animation');
    motionButton.innerHTML = paused
      ? '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 5 10 7-10 7Z"/></svg>'
      : '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6v12M15 6v12"/></svg>';
  }
  updateMotionButton();
  motionButton.addEventListener('click', () => { paused = !paused; updateMotionButton(); });
  reducedMotion.addEventListener('change', (event) => { paused = event.matches; updateMotionButton(); });
  document.querySelectorAll('.color-swatch').forEach((button) => {
    button.addEventListener('click', () => {
      const colors = palettes[button.dataset.color];
      [green, lightGreen, bellyMaterial].forEach((material, index) => material.color.setHex(colors[index]));
      document.querySelectorAll('.color-swatch').forEach((swatch) => {
        const selected = swatch === button;
        swatch.classList.toggle('active', selected);
        swatch.setAttribute('aria-pressed', String(selected));
      });
    });
  });
  document.querySelector('#reset-view').addEventListener('click', () => { targetYaw = -0.32; });
  hopButton.addEventListener('click', () => {
    if (hopStart !== null) return;
    hopStart = performance.now();
    hopButton.disabled = true;
    hopButton.firstChild.textContent = 'Zoom ';
  });
  canvas.addEventListener('pointerdown', (event) => {
    dragging = true;
    lastX = event.clientX;
    canvas.setPointerCapture(event.pointerId);
  });
  canvas.addEventListener('pointermove', (event) => {
    if (!dragging) return;
    targetYaw += (event.clientX - lastX) * 0.008;
    lastX = event.clientX;
  });
  const stopDrag = () => { dragging = false; };
  canvas.addEventListener('pointerup', stopDrag);
  canvas.addEventListener('pointercancel', stopDrag);
  canvas.addEventListener('lostpointercapture', stopDrag);
  canvas.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      event.preventDefault();
      targetYaw += event.key === 'ArrowLeft' ? -0.15 : 0.15;
    }
  });

  function resize() {
    const { width, height } = canvas.getBoundingClientRect();
    if (!width || !height) return;
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    const mobile = window.innerWidth < 900;
    camera.position.set(0, mobile ? 6.3 : 5.6, mobile ? 11.8 : 11.3);
    camera.lookAt(0, 0.7, 0);
    camera.setViewOffset(width, height, mobile ? 0 : -width * 0.105, mobile ? 0 : -height * 0.06, width, height);
    camera.updateProjectionMatrix();
  }
  const observer = new ResizeObserver(resize);
  observer.observe(canvas);
  resize();

  function animate(now) {
    if (disposed) return;
    const delta = Math.min((now - lastTime) / 1000, 0.05);
    lastTime = now;
    if (!paused) elapsed += delta;
    yaw += (targetYaw - yaw) * (1 - Math.exp(-delta * 10));
    world.rotation.y = yaw;
    frog.scale.y = paused ? 1 : 1 + Math.sin(elapsed * 1.7) * 0.018;
    frog.rotation.z = paused ? 0 : Math.sin(elapsed * 0.8) * 0.01;
    if (hopStart !== null) {
      const progress = Math.min((now - hopStart) / 900, 1);
      frog.position.y = reducedMotion.matches ? 0 : Math.sin(progress * Math.PI) * 1.3;
      frog.rotation.x = reducedMotion.matches ? 0 : Math.sin(progress * Math.PI * 2) * 0.12;
      if (progress === 1) {
        hopStart = null;
        frog.position.y = 0;
        frog.rotation.x = 0;
        hopButton.disabled = false;
        hopButton.firstChild.textContent = 'Watch it leap ';
      }
    }
    renderer.render(scene, camera);
    frame = requestAnimationFrame(animate);
  }
  frame = requestAnimationFrame(animate);
  canvas.addEventListener('webglcontextlost', (event) => {
    event.preventDefault();
    disposed = true;
    cancelAnimationFrame(frame);
    showSceneError(new Error('WebGL context lost. Reload the page to restore the pond.'));
  });
  window.addEventListener('pagehide', (event) => {
    if (event.persisted) return;
    disposed = true;
    cancelAnimationFrame(frame);
    observer.disconnect();
    scene.traverse((object) => {
      object.geometry?.dispose();
      if (Array.isArray(object.material)) object.material.forEach((material) => material.dispose());
      else object.material?.dispose();
    });
    renderer.dispose();
  });
}

try {
  createPond();
} catch (error) {
  showSceneError(error);
}
