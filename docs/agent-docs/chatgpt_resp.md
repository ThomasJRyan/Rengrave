Yes. I looked through the current [Rengrave repository](https://github.com/ThomasJRyan/Rengrave?utm_source=chatgpt.com), particularly the `rengrave-ui`, `rengrave-core`, preview, geometry, SVG, and DXF code.

**My conclusion is that you should not build the vector editor entirely from scratch.** Your existing architecture is actually a pretty good starting point for adding one, but I would change what you consider the "source of truth" for geometry.

### The most important observation

Right now Rengrave is fundamentally **toolpath-preview oriented**.

For example, `PreviewSegment` is essentially:

```text
start Point → end Point
```

and your input preview converts vectors into collections of these segments. The main preview then paints those segments directly with `egui::Painter`. 

That's perfectly reasonable for displaying G-code, but it becomes awkward when you want VCarve-like operations:

* click a node
* drag a node
* drag a Bézier handle
* select multiple nodes
* move an entire contour
* insert a node
* delete a node
* convert line ↔ curve
* join/break contours
* offset
* trim
* weld
* boolean union/difference
* snap to grid
* snap to endpoints/intersections
* undo/redo

Those operations want **actual vector geometry**, not a flattened collection of line segments.

The good news is that you already have one of the pieces I'd recommend: **Kurbo**. Your workspace currently uses `kurbo = "0.13.1"`. 

---

# What I'd build

I'd make the architecture roughly:

```text
                         ┌──────────────────────┐
                         │    Rengrave UI       │
                         │       egui           │
                         └──────────┬───────────┘
                                    │
                            mouse / keyboard
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │   Vector Editor      │
                         │                      │
                         │ selection            │
                         │ dragging             │
                         │ snapping              │
                         │ transforms           │
                         │ commands / undo       │
                         └──────────┬───────────┘
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │   Vector Document    │
                         │                      │
                         │ contours             │
                         │ segments             │
                         │ Béziers              │
                         │ arcs                  │
                         │ nodes                 │
                         └───────┬─────────┬────┘
                                 │         │
                    ┌────────────┘         └────────────┐
                    ▼                                   ▼
             ┌─────────────┐                     ┌──────────────┐
             │   Kurbo      │                     │  iOverlay    │
             │ geometry    │                     │ booleans     │
             └─────────────┘                     │ offsets      │
                                                  │ clipping     │
                                                  └──────────────┘
                    │
                    ▼
             ┌────────────────┐
             │ Rengrave core  │
             │ toolpath       │
             │ generation     │
             └────────────────┘
```

This is much closer to how I'd approach turning Rengrave into a small VCarve-like application.

---

# 1. Use Kurbo as your vector geometry layer

[Kurbo documentation](https://docs.rs/kurbo/latest/kurbo/?utm_source=chatgpt.com) is specifically designed around 2D curves and vector paths. It provides Bézier curves, paths, geometric operations, intersections, etc. ([Docs.rs][1])

That makes it a substantially better foundation for this than inventing your own Bézier mathematics.

For example, conceptually your document could contain:

```rust
struct VectorDocument {
    contours: Vec<Contour>,
}

struct Contour {
    segments: Vec<Segment>,
    closed: bool,
}

enum Segment {
    Line {
        start: Point,
        end: Point,
    },
    Quadratic {
        start: Point,
        control: Point,
        end: Point,
    },
    Cubic {
        start: Point,
        control1: Point,
        control2: Point,
        end: Point,
    },
    Arc {
        // ...
    },
}
```

But I wouldn't necessarily expose Kurbo's types directly as your public document representation.

I'd make a Rengrave-specific representation and convert to/from Kurbo.

That gives you freedom later.

---

# 2. Don't make `PreviewSegment` your editable geometry

This is probably the single biggest architectural change I'd make.

Currently you essentially have:

```text
SVG/DXF/font
      ↓
Vec<PreviewSegment>
      ↓
egui Painter
```

Instead:

```text
SVG/DXF/font
      ↓
VectorDocument
      ↓
 ┌──────────────┬───────────────┐
 ↓              ↓               ↓
Editor       Renderer       Toolpath
 ↓              ↓               ↓
selection     egui          CNC geometry
```

Your renderer can still flatten curves into line segments when necessary.

For example:

```text
CubicBezier
    ↓
Kurbo
    ↓
flatten
    ↓
PreviewSegment[]
```

That's essentially what your application is already doing conceptually, but you'd move the flattening **later** in the pipeline.

This distinction is extremely important.

---

# 3. egui is actually sufficient for the editor

I don't think you need to abandon egui.

Your current preview already has most of the infrastructure required:

* model → screen transformation
* screen → model transformation
* zoom
* pan
* grid
* coordinate readout
* `egui::Painter`
* custom interaction regions

Your `screen_point_to_model()` is particularly useful here. 

And egui itself already supports paths and quadratic/cubic Béziers through its painting API. ([Docs.rs][2])

So I'd keep:

**egui/eframe = UI and interaction**

rather than trying to introduce another GUI framework.

Your existing design philosophy also fits this extremely well: you already describe the central preview as the dominant interactive canvas with grid, axes, bounds, toolpaths and input outlines. 

---

# 4. I'd introduce an actual `VectorEditor` widget

Rather than making `preview.rs` progressively more complicated, I'd create something like:

```text
rengrave-ui/src/
    preview.rs
    vector_editor.rs
    vector_editor/
        document.rs
        selection.rs
        interaction.rs
        snapping.rs
        rendering.rs
        commands.rs
```

Then your main application would eventually have something like:

```rust
VectorEditor::new(&mut self.vector_document)
    .show(ui);
```

Internally:

```rust
pub struct VectorEditor {
    selection: Selection,
    interaction: InteractionState,
    snap: SnapSettings,
    tool: EditorTool,
}
```

with:

```rust
enum EditorTool {
    Select,
    Node,
    DrawLine,
    DrawBezier,
    Rectangle,
    Circle,
    Trim,
    Offset,
}
```

This gives you the VCarve-style "tool mode" concept without contaminating the rest of your application.

---

# 5. Selection should be its own subsystem

This is one area where trying to do everything directly inside an egui callback tends to become painful.

I'd explicitly model:

```rust
enum Selection {
    None,
    Nodes(Vec<NodeId>),
    Segments(Vec<SegmentId>),
    Contours(Vec<ContourId>),
}
```

or preferably allow combinations:

```rust
struct Selection {
    nodes: HashSet<NodeId>,
    segments: HashSet<SegmentId>,
    contours: HashSet<ContourId>,
}
```

Then your interaction becomes:

```text
mouse down
    ↓
hit testing
    ↓
what did we hit?
    ↓
node / handle / segment / contour / empty
    ↓
selection update
    ↓
drag operation
```

This is much easier to reason about than:

```rust
if response.dragged() {
    ...
}
```

everywhere.

---

# 6. Node IDs are important

Don't identify nodes by their position in a `Vec`.

I'd use something like:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u64);
```

Then:

```rust
struct Node {
    id: NodeId,
    position: Point,
    incoming: Handle,
    outgoing: Handle,
}
```

This becomes extremely useful for:

* selection
* undo/redo
* dragging
* topology changes
* joining paths
* deleting nodes
* inserting nodes

It also prevents the usual nightmare where deleting element 3 changes every index after it.

---

# 7. Bézier editing becomes surprisingly straightforward

For a cubic:

```text
P0 ─────────────── P3
 \                /
  P1              P2
```

you have:

```rust
struct CubicNode {
    position: Point,
    incoming: Option<Point>,
    outgoing: Option<Point>,
}
```

Your renderer draws:

```text
P0 → P1 → P2 → P3
```

and your editor renders:

```text
P0 ●──────────────● P3
   ╲              ╱
    ○            ○
```

The control handles are simply little draggable egui interaction targets.

Your existing coordinate transform does almost all of the hard work.

---

# 8. Make snapping a first-class subsystem

VCarve-like editing lives or dies on snapping.

I'd create:

```rust
struct SnapSettings {
    grid: bool,
    endpoints: bool,
    midpoints: bool,
    intersections: bool,
    centers: bool,
    tangent: bool,
}
```

and:

```rust
fn snap_point(
    document: &VectorDocument,
    point: Point,
    settings: &SnapSettings,
) -> Point
```

The important detail is:

**snap in model space, not screen space.**

Your screen/model conversion already makes this feasible. 

For grid snapping:

```rust
x = (x / grid_size).round() * grid_size;
y = (y / grid_size).round() * grid_size;
```

For endpoint snapping, search nearby model-space nodes.

And because you know the current zoom, you can define the hit radius in pixels and convert it into model units.

---

# 9. iOverlay is very interesting for Rengrave

This is the other crate I'd seriously consider.

[iOverlay documentation](https://docs.rs/i_overlay/latest/i_overlay/?utm_source=chatgpt.com) provides:

* union
* intersection
* difference
* XOR
* self-intersection handling
* polygon clipping
* slicing
* offsets/buffering
* holes
* multiple contours
* simplification

and is explicitly aimed at CAD/graphics/GIS-style geometry. ([Docs.rs][3])

That's **extremely relevant to CNC software**.

For example:

```text
        ┌───────┐
        │       │
        │   ┌───┼───┐
        │   │   │   │
        └───┼───┘   │
            └───────┘

              ↓ UNION

        ┌─────────────┐
        │             │
        │             │
        └─────────────┘
```

Or:

```text
rectangle
    -
circle
    ↓
difference
    ↓
pocket/profile geometry
```

That's precisely the kind of operation you'll eventually want for a VCarve-style workflow.

---

# 10. You already have `clipper2`

This is interesting because your Cargo workspace already includes:

```toml
clipper2 = { version = "0.6.0", default-features = false }
```



So **I would not immediately replace it.**

You effectively have three choices:

| Library      | Best use                                  |
| ------------ | ----------------------------------------- |
| **Kurbo**    | Curves, Béziers, vector geometry          |
| **clipper2** | CNC polygon offsets/booleans              |
| **iOverlay** | Modern Rust polygon boolean/offset engine |

There is also `geo_clipper`, which wraps the C++ Clipper implementation and provides intersection/difference/union/XOR/offset operations. ([Docs.rs][4])

My instinct for Rengrave would be:

**Kurbo + your existing clipper2 initially.**

Investigate iOverlay later if clipper2 becomes limiting.

---

# 11. Don't use polygon booleans directly on Béziers

There's an important distinction here.

A VCarve-style editor operates on:

```text
lines
arcs
Bézier curves
```

while many boolean engines operate on:

```text
polygons / polylines
```

So your pipeline should probably be:

```text
              Editable representation
                       │
           ┌───────────┴───────────┐
           ↓                       ↓
      Rendering                CNC geometry
           │                       │
           ↓                       ↓
     Kurbo curves             flatten/offset
                                   │
                                   ↓
                            Clipper/iOverlay
```

**Don't destroy the original curves just because you need polygons for a machining operation.**

That's one of the easiest mistakes to make in a vector editor.

---

# 12. Your SVG/DXF code is actually a good place to start

You already have dedicated SVG and DXF processing in `rengrave-core`. ([GitHub][5])

I'd change the import pipeline from:

```text
SVG
 ↓
segments
 ↓
preview
```

to:

```text
SVG
 ↓
VectorDocument
 ↓
editor
```

and similarly:

```text
DXF
 ↓
VectorDocument
 ↓
editor
```

Then the CNC generator consumes the same document.

That gives you a very nice architecture:

```text
                   VectorDocument
                         │
              ┌──────────┼──────────┐
              │          │          │
              ▼          ▼          ▼
           Preview      SVG       DXF
              │
              ▼
          Toolpath
              │
              ▼
             GCode
```

---

# 13. I would actually add a "Design" workbench

This is where I think Rengrave could start feeling dramatically more like VCarve without destroying its existing workflow.

Currently your application is essentially:

```text
Input
 ↓
Settings
 ↓
Calculate
 ↓
Preview
 ↓
Output
```

I'd introduce:

```text
┌─────────────────────────────────────────────────────────┐
│ File   Edit   View   Geometry   Toolpath   Help          │
├─────────────────────────────────────────────────────────┤
│ Select │ Node │ Line │ Bezier │ Rectangle │ Circle ...  │
├───────────────┬─────────────────────────┬───────────────┤
│               │                         │               │
│ Objects       │                         │ Properties    │
│               │       CANVAS            │               │
│ ○ Contour 1   │                         │ X: 42.50      │
│ ○ Contour 2   │       vector            │ Y: 18.20      │
│ ○ Text        │       geometry          │ W: 80.00      │
│               │                         │ H: 25.00      │
│               │                         │               │
├───────────────┴─────────────────────────┴───────────────┤
│ X 42.500  Y 18.200       Snap: Grid 2mm       100%     │
└─────────────────────────────────────────────────────────┘
```

Then:

**Design → Toolpath**

becomes a natural workflow.

---

# 14. The really nice part: your existing preview can remain

I wouldn't throw away `preview.rs`.

I'd make it responsible for **toolpath visualization**, while the new editor becomes responsible for **source geometry**.

Something like:

```text
                    RengraveApp
                         │
            ┌────────────┴────────────┐
            │                         │
            ▼                         ▼
      VectorEditor              ToolpathPreview
            │                         │
            ▼                         ▼
     VectorDocument              G-code
            │                         │
            └────────────┬────────────┘
                         ▼
                       Canvas
```

The existing preview code already has a lot of useful functionality—grid, coordinate transformation, fit-to-bounds, zoom-at-cursor, scale bars, etc. 

I'd actually **extract those pieces into a reusable `Canvas2D` abstraction**.

---

# 15. Something like this would be my target architecture

```rust
pub struct Canvas2D {
    pub transform: ViewTransform,
    pub rect: egui::Rect,
}

impl Canvas2D {
    pub fn screen_to_model(&self, p: egui::Pos2) -> Point;
    pub fn model_to_screen(&self, p: Point) -> egui::Pos2;

    pub fn zoom_at(&mut self, screen: egui::Pos2, factor: f64);
    pub fn pan(&mut self, delta: egui::Vec2);
    pub fn fit(&mut self, bounds: Bounds);
}
```

Then:

```rust
pub struct VectorEditor {
    document: VectorDocument,
    selection: Selection,
    tool: EditorTool,
    canvas: Canvas2D,
    snap: SnapSettings,
}
```

And:

```rust
pub struct ToolpathPreview {
    canvas: Canvas2D,
    ...
}
```

Both share the same coordinate system.

---

# 16. Undo/redo is worth designing now

Don't bolt this on later.

Use command-based editing:

```rust
trait EditCommand {
    fn execute(&mut self, doc: &mut VectorDocument);
    fn undo(&mut self, doc: &mut VectorDocument);
}
```

Then:

```rust
MoveNodesCommand
InsertNodeCommand
DeleteNodeCommand
SplitSegmentCommand
JoinContoursCommand
TransformCommand
BooleanCommand
```

Your application gets:

```text
Ctrl+Z
Ctrl+Y
```

almost automatically.

Even better, commands can become the boundary between your UI and geometry engine.

---

# 17. There is an existing `vector_editor_core` crate, but...

I did find a Rust crate called `vector_editor_core`. It provides point objects, layers, selection settings and selectable point structures. ([Docs.rs][6])

I **wouldn't build Rengrave around it**.

It looks interesting as inspiration, but Rengrave has substantially more specialized requirements:

* CNC geometry
* arcs
* toolpaths
* DXF
* SVG
* offsets
* v-carving
* units
* machining tolerances
* topology

You'd likely spend more time fighting its abstractions than you'd save.

---

# 18. One particularly important change: separate geometry from toolpath

Your current project has a nice core/UI split already:

```text
rengrave-core
rengrave-ui
rengrave-cli
rengrave-potrace
```

The README explicitly describes `rengrave-core` as containing geometry, parsers, toolpath generation and exporters, while the UI is `eframe/egui`. ([GitHub][7])

I'd extend that architecture rather than radically restructure it:

```text
rengrave-core
│
├── geometry/
│   ├── point
│   ├── path
│   ├── segment
│   ├── contour
│   └── transform
│
├── vector/
│   ├── document
│   ├── node
│   ├── selection
│   ├── snapping
│   ├── boolean
│   └── offset
│
├── svg/
├── dxf/
├── toolpath/
├── vcarve/
└── ...
```

The UI then knows about the vector document, but **doesn't own the actual geometry mathematics**.

---

# What I would use

If I were implementing this in Rengrave today, my stack would be:

### Keep

* **egui / eframe** — UI
* **your `ViewTransform` concept** — coordinate transformation
* **your existing preview renderer**
* **your existing `clipper2` integration**
* **your SVG/DXF infrastructure**

### Add/use heavily

* **Kurbo** — primary curve/vector mathematics
* **custom `VectorDocument`** — editable source geometry
* **custom selection/hit-testing system**
* **custom snapping system**
* **command-based undo/redo**
* **egui Painter** — rendering/editor handles
* **iOverlay** — investigate for future boolean/offset functionality

### Don't use

* a full game engine
* Bevy
* another GUI framework
* a giant CAD kernel

I think those would make Rengrave considerably more complicated without solving the particular problem you're facing.

---

## The biggest practical change

If you want to get this working without disappearing into a six-month rewrite, I would **not start with "build a VCarve clone."**

I'd implement these six things in this order:

```text
1. VectorDocument
       ↓
2. Canvas2D abstraction
       ↓
3. Select + hit testing
       ↓
4. Drag nodes
       ↓
5. Bézier handles
       ↓
6. Undo/redo
```

Once those work, you suddenly have a legitimate vector editor.

Then:

```text
7. Rectangle / circle / line tools
8. Multi-select
9. Move / rotate / scale
10. Grid snapping
11. Endpoint snapping
12. Insert/delete nodes
13. Join / break contours
14. Offset
15. Boolean operations
16. Trim
17. Toolpath generation directly from edited geometry
```

At that point Rengrave starts becoming a **CNC-oriented vector editor**, rather than an F-Engrave replacement with an increasingly complicated preview window.

And I think that's actually a very good direction for the project.

**One other thing I'd strongly recommend:** don't try to make the editor operate on the current `Vec<PreviewSegment>` representation. Your existing code is already sophisticated enough that doing so would probably lead to exactly the kind of frustration you've been experiencing. The current `PreviewSegment` architecture is excellent for *displaying* toolpaths, but it's the wrong abstraction for *editing* vector geometry. 

If you want to take this further, I can also go through the repository file-by-file and propose a **concrete Rengrave vector-editor architecture**, including the structs, modules, Kurbo integration, egui mouse interaction model, and how I'd modify your current `preview.rs`/`input_preview.rs` rather than designing it in the abstract.

[1]: https://docs.rs/kurbo?utm_source=chatgpt.com "kurbo - Rust"
[2]: https://docs.rs/egui/latest/egui/enum.Shape.html?utm_source=chatgpt.com "Shape in egui - Rust"
[3]: https://docs.rs/crate/i_overlay/latest?utm_source=chatgpt.com "i_overlay 7.0.2 - Docs.rs"
[4]: https://docs.rs/geo-clipper/latest/geo_clipper/?utm_source=chatgpt.com "geo_clipper - Rust"
[5]: https://github.com/ThomasJRyan/Rengrave/tree/main/crates/rengrave-core/src "Rengrave/crates/rengrave-core/src at main · ThomasJRyan/Rengrave · GitHub"
[6]: https://docs.rs/vector-editor-core/latest/vector_editor_core/?utm_source=chatgpt.com "vector_editor_core - Rust"
[7]: https://github.com/ThomasJRyan/Rengrave "GitHub - ThomasJRyan/Rengrave · GitHub"

