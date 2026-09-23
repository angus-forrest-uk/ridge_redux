import { For, Index, Show, type JSX } from "solid-js";
import { COLOR_SUGGESTIONS, toHex } from "../lib/colors.ts";
import { clampLat, type Params } from "../lib/params.ts";
import type { Bbox } from "../lib/pipeline.ts";
import { useRidge } from "../state.ts";

type NumberKey = { [K in keyof Params]-?: Params[K] extends number ? K : never }[keyof Params];
type BooleanKey = { [K in keyof Params]-?: Params[K] extends boolean ? K : never }[keyof Params];

function Section(props: { title: string; children: JSX.Element }) {
  return (
    <div class="section">
      <h2>{props.title}</h2>
      {props.children}
    </div>
  );
}

function Row(props: { label: string; children: JSX.Element }) {
  return (
    <div class="row">
      <label>{props.label}</label>
      {props.children}
    </div>
  );
}

function Slider(props: { label: string; key: NumberKey; min: number; max: number; step: number }) {
  const { params, set } = useRidge();
  return (
    <Row label={props.label}>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={params[props.key]}
        onInput={(e) => set(props.key, Number(e.currentTarget.value))}
      />
      <span class="val">{params[props.key]}</span>
    </Row>
  );
}

function Toggle(props: { label: string; key: BooleanKey; hint: string }) {
  const { params, set } = useRidge();
  return (
    <div class="row">
      <label for={props.key}>{props.label}</label>
      <input
        id={props.key}
        type="checkbox"
        checked={params[props.key]}
        onChange={(e) => set(props.key, e.currentTarget.checked)}
      />
      <span class="hint" tabIndex={0} title={props.hint} aria-label={props.hint}>?</span>
    </div>
  );
}

function Select<K extends keyof Params>(props: {
  label: string;
  key: K;
  options: [Params[K], string][];
}) {
  const { params, set } = useRidge();
  return (
    <Row label={props.label}>
      <select
        onChange={(e) => {
          const [value] = props.options.find(([v]) => String(v) === e.currentTarget.value)!;
          set(props.key, value);
        }}
      >
        <For each={props.options}>
          {([value, text]) => (
            <option value={String(value)} selected={params[props.key] === value}>{text}</option>
          )}
        </For>
      </select>
    </Row>
  );
}

function BboxInputs() {
  const { params, setBbox } = useRidge();
  return (
    <div class="row bbox">
      <Index each={params.bbox}>
        {(value, i) => (
          <input
            type="number"
            step="0.000001"
            value={value()}
            onChange={(e) => {
              const v = Number(e.currentTarget.value);
              const bbox = [...params.bbox] as Bbox;
              // Latitudes stay inside SRTM coverage.
              bbox[i] = i % 2 === 1 ? clampLat(v) : v;
              e.currentTarget.value = String(bbox[i]);
              setBbox(bbox);
            }}
          />
        )}
      </Index>
    </div>
  );
}

function LineColor() {
  const { params, set } = useRidge();
  return (
    <Row label="line color">
      <input
        type="text"
        list="colors"
        value={params.line_color}
        onChange={(e) => set("line_color", e.currentTarget.value)}
      />
      <datalist id="colors">
        <For each={COLOR_SUGGESTIONS}>{(name) => <option value={name} />}</For>
      </datalist>
    </Row>
  );
}

function Background() {
  const { params, set } = useRidge();
  return (
    <Row label="background">
      <input
        type="color"
        value={toHex(params.background_color)}
        onInput={(e) => set("background_color", e.currentTarget.value)}
      />
      <input
        type="text"
        value={params.background_color}
        onChange={(e) => set("background_color", e.currentTarget.value)}
      />
    </Row>
  );
}

function Annotation() {
  const { params, set } = useRidge();
  let lon!: HTMLInputElement, lat!: HTMLInputElement, text!: HTMLInputElement;
  const update = () => {
    set("annotation", lon.value === "" || lat.value === "" ? null : {
      lon: Number(lon.value),
      lat: Number(lat.value),
      label: text.value,
      x_offset: 0.005,
      y_offset: 0.005,
      label_size_pt: 20,
      dot_pt: 8,
      color: "#ffffff",
      background: false,
    });
  };
  return (
    <div>
      <div class="row">
        <input ref={lon} type="number" step="0.0001" placeholder="lon" value={params.annotation?.lon ?? ""} onChange={update} />
        <input ref={lat} type="number" step="0.0001" placeholder="lat" value={params.annotation?.lat ?? ""} onChange={update} />
      </div>
      <div class="row">
        <input ref={text} type="text" placeholder="label" value={params.annotation?.label ?? ""} onChange={update} />
        <button
          onClick={() => {
            lon.value = lat.value = text.value = "";
            set("annotation", null);
          }}
        >
          clear
        </button>
      </div>
    </div>
  );
}

export default function Controls() {
  const { params, set } = useRidge();
  return (
    <aside id="controls">
      <Section title="location">
        <BboxInputs />
        <Select label="region" key="region" options={[["rect", "rectangle (orbiting window)"], ["disc", "full disc"]]} />
        <Show when={params.region === "disc"}>
          <Slider label="region span (deg)" key="span_deg" min={0.05} max={5} step={0.01} />
        </Show>
      </Section>
      <Section title="viewpoint">
        <Slider label="angle (deg)" key="viewpoint_angle" min={0} max={360} step={1} />
        <Select label="interpolation" key="interpolation" options={[[0, "nearest (0)"], [1, "bilinear (1)"]]} />
      </Section>
      <Section title="resolution">
        <Slider label="num lines" key="num_lines" min={10} max={400} step={5} />
        <Slider label="pts / line" key="elevation_pts" min={20} max={1000} step={10} />
      </Section>
      <Section title="water & relief">
        <Slider label="water ntile" key="water_ntile" min={0} max={100} step={1} />
        <Slider label="lake flatness" key="lake_flatness" min={0} max={10} step={1} />
        <Slider label="vertical ratio" key="vertical_ratio" min={5} max={400} step={5} />
        <Toggle
          label="clip frame to land"
          key="clip_to_land"
          hint="Legacy compatibility: the original ridge_map lets matplotlib frame the axes around the land it draws, so water and tiles with no data crop the picture — and the water ntile rescales it. On matches that; off (the default) frames the whole selected area, so masking never moves the frame."
        />
      </Section>
      <Section title="style">
        <LineColor />
        <Select label="colormap kind" key="kind" options={[["gradient", "gradient"], ["elevation", "elevation"]]} />
        <Slider label="linewidth (pt)" key="linewidth_pt" min={0.5} max={10} step={0.5} />
        <Background />
        <Slider label="size scale" key="size_scale" min={8} max={40} step={1} />
      </Section>
      <Section title="label">
        <textarea rows={2} value={params.label} onChange={(e) => set("label", e.currentTarget.value)} />
        <Slider label="label x" key="label_x" min={0} max={1} step={0.01} />
        <Slider label="label y" key="label_y" min={0} max={1} step={0.01} />
        <Slider label="label size" key="label_size_pt" min={10} max={120} step={2} />
      </Section>
      <Section title="annotation">
        <Annotation />
      </Section>
      <Section title="about">
        <p>Rotation, water, relief and style update locally. Only the location and resolution reach the server.</p>
      </Section>
    </aside>
  );
}
