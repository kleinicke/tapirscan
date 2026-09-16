import { mount } from "svelte";
import Demo from "./Demo.svelte";
import "./style.css";

// Page-view analytics load only when a build sets VITE_PLAUSIBLE_SRC, so forks don't report to our server.
const plausible = import.meta.env.VITE_PLAUSIBLE_SRC;
if (plausible) {
  const script = document.createElement("script");
  script.defer = true;
  script.dataset.domain = "tapirscan.netlify.app";
  script.src = plausible;
  document.head.append(script);
}

mount(Demo, { target: document.getElementById("app")! });
