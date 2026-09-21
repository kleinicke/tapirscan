import { mount } from "svelte";
import Demo from "./Demo.svelte";
import "./style.css";

// Keep forks, local development and deploy previews out of production analytics.
// The dashboard identifier stays unchanged so both hostnames share its history.
const analyticsHosts = ["tapirscan.netlify.app", "tapirscan.f-kleinicke.de"];
if (analyticsHosts.includes(window.location.hostname)) {
  const script = document.createElement("script");
  script.defer = true;
  script.dataset.domain = "tapirscan.netlify.app";
  script.src = "https://analytics.re4vive.com/js/script.js";
  document.head.append(script);
}

const target = document.getElementById("app");
if (!target) throw new Error("Demo mount element is missing");
mount(Demo, { target });
