import { render } from "solid-js/web";

import { App } from "./App";
import { connect } from "./backend";
import "./style.css";

const backend = await connect();
render(() => <App backend={backend} initial={backend.boot.model} />, document.getElementById("root")!);
