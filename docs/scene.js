import * as THREE from 'three';
import { createSanbornFrog } from '../src/frog.js';

export function mount() {
  const host = document.querySelector('.specimen');
  const canvas = document.querySelector('#specimen');
  const button = document.querySelector('#motion');
  const preference = matchMedia('(prefers-reduced-motion: reduce)');
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 1.5));
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.35;
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(35, 1, 0.1, 50);
  camera.position.set(0, 4.6, 9.8);
  camera.lookAt(0, 1.05, 0);
  scene.add(new THREE.HemisphereLight(0xfffbed, 0x6c7958, 2.5));
  const key = new THREE.DirectionalLight(0xfff5dc, 4);
  key.position.set(-3, 8, 5);
  scene.add(key);
  const rim = new THREE.DirectionalLight(0xcddfc7, 1.1);
  rim.position.set(6, 4, -3);
  scene.add(rim);
  const { frog } = createSanbornFrog();
  frog.rotation.y = -0.32;
  scene.add(frog);
  const rings = new THREE.Group();
  rings.position.y = -0.06;
  scene.add(rings);
  for (let i = 0; i < 3; i++) {
    const radius = 2.2 + i * 0.4;
    const points = Array.from({ length: 120 }, (_, j) => {
      const angle = j / 120 * Math.PI * 2;
      return new THREE.Vector3(Math.cos(angle) * radius, 0, Math.sin(angle) * radius);
    });
    rings.add(new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(points), new THREE.LineBasicMaterial({ color: 0x9ab875, transparent: true, opacity: 0.3 - i * 0.06 })));
  }
  let paused = preference.matches;
  let visible = true;
  let failed = false;
  let frame = 0;
  let elapsed = 0;
  let previous = null;
  function draw(time) {
    frame = 0;
    if (failed) return;
    if (previous !== null && !paused) elapsed += Math.min((time - previous) / 1000, 0.05);
    previous = time;
    frog.rotation.y = -0.32 + (paused ? 0 : Math.sin(elapsed * 0.2) * 0.06);
    frog.scale.y = paused ? 1 : 1 + Math.sin(elapsed * 1.7) * 0.018;
    rings.rotation.y = elapsed * 0.03;
    renderer.render(scene, camera);
    if (!paused && visible && !document.hidden) frame = requestAnimationFrame(draw);
  }
  function refresh() {
    cancelAnimationFrame(frame);
    previous = null;
    if (visible && !document.hidden && !failed) frame = requestAnimationFrame(draw);
  }
  function controls() {
    button.textContent = paused ? 'Resume sculpture' : 'Pause sculpture';
    button.setAttribute('aria-pressed', String(paused));
    refresh();
  }
  button.hidden = false;
  button.addEventListener('click', () => { paused = !paused; controls(); });
  preference.addEventListener('change', () => { paused = preference.matches; controls(); });
  const resize = new ResizeObserver(() => {
    if (!host.clientWidth || !host.clientHeight) return;
    renderer.setSize(host.clientWidth, host.clientHeight, false);
    camera.aspect = host.clientWidth / host.clientHeight;
    camera.updateProjectionMatrix();
    refresh();
  });
  resize.observe(host);
  const observer = new IntersectionObserver(entries => { visible = entries[0].isIntersecting; refresh(); });
  observer.observe(host);
  document.addEventListener('visibilitychange', refresh);
  canvas.addEventListener('webglcontextlost', event => {
    event.preventDefault();
    failed = true;
    cancelAnimationFrame(frame);
    host.classList.remove('loaded');
    button.hidden = true;
    document.querySelector('#scene-error').hidden = false;
    console.error('Sanborn sculpture: WebGL context lost.');
  });
  addEventListener('pagehide', event => {
    if (event.persisted) return;
    failed = true;
    cancelAnimationFrame(frame);
    resize.disconnect();
    observer.disconnect();
    scene.traverse(object => {
      object.geometry?.dispose();
      object.material?.dispose();
    });
    renderer.dispose();
  });
  host.classList.add('loaded');
  controls();
}
