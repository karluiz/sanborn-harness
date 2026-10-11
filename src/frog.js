import * as THREE from 'three';

export function createSanbornFrog() {
  const frog = new THREE.Group();
  const green = new THREE.MeshStandardMaterial({ color: 0x6f9252, roughness: 0.88, flatShading: true });
  const lightGreen = new THREE.MeshStandardMaterial({ color: 0x8da969, roughness: 0.9, flatShading: true });
  const bellyMaterial = new THREE.MeshStandardMaterial({ color: 0xc6d39b, roughness: 1, flatShading: true });
  const eyeMaterial = new THREE.MeshStandardMaterial({ color: 0xf2efcf, roughness: 0.5, flatShading: true });
  const darkMaterial = new THREE.MeshStandardMaterial({ color: 0x233025, roughness: 0.45 });
  const highlightMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff });
  function mesh(geometry, material, position, scale) {
    const object = new THREE.Mesh(geometry, material);
    object.position.set(...position);
    if (scale) object.scale.set(...scale);
    object.castShadow = true;
    object.receiveShadow = true;
    frog.add(object);
    return object;
  }
  function pebble(material, position, scale, detail = 1) {
    return mesh(new THREE.IcosahedronGeometry(1, detail), material, position, scale);
  }
  function limb(start, end, radius, material = green) {
    const a = new THREE.Vector3(...start);
    const b = new THREE.Vector3(...end);
    const cylinder = mesh(new THREE.CylinderGeometry(radius * 0.8, radius, a.distanceTo(b), 5), material, a.clone().add(b).multiplyScalar(0.5).toArray());
    cylinder.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), b.sub(a).normalize());
  }
  pebble(green, [0, 1.02, -0.13], [1.1, 0.95, 0.93]);
  pebble(bellyMaterial, [0, 0.9, 0.53], [0.82, 0.76, 0.47]);
  pebble(green, [0, 1.75, 0.34], [1.2, 0.67, 0.87]);
  pebble(lightGreen, [0, 1.47, 0.78], [0.97, 0.31, 0.49]);
  for (const side of [-1, 1]) {
    pebble(green, [side * 0.75, 2.12, 0.43], [0.48, 0.49, 0.47]);
    pebble(eyeMaterial, [side * 0.75, 2.15, 0.73], [0.32, 0.34, 0.2], 2);
    pebble(darkMaterial, [side * 0.73, 2.16, 0.905], [0.14, 0.2, 0.065], 2);
    pebble(highlightMaterial, [side * 0.73 - 0.045, 2.23, 0.957], [0.046, 0.056, 0.021]);
    pebble(green, [side * 1.12, 0.56, -0.32], [0.65, 0.61, 0.79]);
    limb([side * 1.32, 0.5, -0.31], [side * 1.45, 0.18, 0.54], 0.22);
    pebble(lightGreen, [side * 1.43, 0.15, 0.62], [0.42, 0.15, 0.44]);
    limb([side * 0.69, 1.19, 0.56], [side * 0.8, 0.6, 0.84], 0.16);
    limb([side * 0.8, 0.6, 0.84], [side * 0.84, 0.16, 1.14], 0.12, lightGreen);
    pebble(lightGreen, [side * 0.85, 0.13, 1.2], [0.25, 0.12, 0.26]);
    for (let toe = 0; toe < 3; toe++) {
      const x = side * (0.68 + toe * 0.17);
      const z = 1.52 + (toe === 1 ? 0.07 : 0);
      limb([side * 0.83, 0.11, 1.19], [x, 0.08, z], 0.045, lightGreen);
      pebble(lightGreen, [x, 0.08, z], [0.078, 0.045, 0.095], 0);
      limb([side * 1.43, 0.12, 0.75], [side * (1.2 + toe * 0.23), 0.075, 1.04], 0.06, lightGreen);
    }
    pebble(darkMaterial, [side * 0.3, 1.74, 1.127], [0.037, 0.025, 0.02]);
  }
  const smile = [];
  for (let i = 0; i <= 20; i++) {
    const x = (i / 20 - 0.5) * 1.25;
    smile.push(new THREE.Vector3(x, 1.43 + 0.15 * (x / 0.625) ** 2, 1.12 - 0.15 * (x / 0.625) ** 2));
  }
  mesh(new THREE.TubeGeometry(new THREE.CatmullRomCurve3(smile), 20, 0.017, 5, false), darkMaterial, [0, 0, 0]);
  return { frog, green, lightGreen, bellyMaterial };
}
