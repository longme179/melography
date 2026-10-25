# MELOGRAPHY — Error-state inventory (UX_SPEC §9 × ARCHITECTURE §8)

**Version:** M4 session 10 (issue #25) · updated M5-A session 14 (A1 preview transport) · **Scope:** every user-facing error
code in the app · **Enforced by:** `scripts/error_inventory.py` (CI) — the
matrix below, the Rust `AppError::new("…")` literals and the EN/VI i18n
catalogs are cross-checked on every push; a code without a row, a row
without both localizations, or a localized code without a Rust caller
fails the build.

## How errors surface (UX_SPEC §9 "message + cause + next action")

| Surface | Mechanism | Used by |
|---|---|---|
| **Toast** | `App.svelte` global toast (`role="status" aria-live="polite"`, auto-dismiss) — carries the localized `error.code.*` guidance | Most command failures (open/save/export/preset/settings) |
| **Error banner** | `ErrorBanner.svelte` persistent banner with dismiss | Session-level failures that need the user to act before proceeding |
| **Batch queue row** | `BatchQueue.svelte` per-job error line (localized code + retry keeps settings, UX_SPEC §7) | Per-file export failures (`io.disk_full` → folder chooser, `io.permission`, `io.decode`, `lut.missing`, `stock.missing`, `export.failed`) |
| **Amber badge** | Toolbar / film panel badge, non-blocking | `stock.missing` (recipe references a stock id not in the registry) |
| **Inline dialog rows** | Import/model dialogs report their own failure rows | `lut.*` import errors, `model.*` manager errors |

No raw error strings ever reach the UI: the Svelte layer renders
`error.code.*` localized text keyed by the stable code (`tErrorCode`),
with the raw engine message logged to the console for diagnostics only —
CI-enforced by `ui_i18n_audit.mjs` (embedded-string scan).

## The matrix (39 codes)

Taxonomy column maps to ARCHITECTURE §8 classes; codes marked
*app-layer* extend the §8 engine taxonomy (session/preset/settings/batch
have no §8 class — listed honestly instead of force-fitted).

| Code | §8 class | EN guidance | VI guidance | Surfaces | Next action |
|---|---|---|---|---|---|
| `batch.job` | app-layer (queue) | That queue row no longer exists. | Dòng hàng đợi đó không còn tồn tại. | batch row | refresh list |
| `batch.rendering` | app-layer (queue) | The batch job failed while rendering. | Công việc trong hàng đợi thất bại khi dựng hình. | batch row | retry the job (settings kept) |
| `color.transform` | ColorError.TransformFailed | The display color transform failed. | Chuyển đổi màu hiển thị thất bại. | toast | retry; report if persistent |
| `export.failed` | ExportError (generic) | Export failed. Check disk space and permissions. | Xuất ảnh thất bại. Kiểm tra dung lượng ổ đĩa và quyền ghi. | toast, batch row | check disk/permissions, retry |
| `film.invalid_adjustments` | PipelineError.StageFailed | The film adjustments are invalid. | Các điều chỉnh phim không hợp lệ. | toast | fix the highlighted slider |
| `film.invalid_lens` | PipelineError.StageFailed | The lens settings are invalid. | Cài đặt ống kính không hợp lệ. | toast | fix the lens panel values |
| `film.unknown_stock` | PipelineError.StageFailed | That film stock is unknown to this build. | Không nhận dạng được loại phim trong bản build này. | toast, amber badge | pick another stock |
| `io.bomb` | IoError.DecompressionBomb | The image dimensions exceed the safety limit. | Kích thước ảnh vượt giới hạn an toàn. | toast | use a smaller image |
| `io.decode` | IoError.Unreadable/Truncated | The file is damaged or truncated and cannot be decoded. | Tệp bị hỏng hoặc thiếu dữ liệu, không thể giải mã. | toast, batch row (`hint.fileUnsupported`) | re-check the source file |
| `io.dest_missing` | ExportError | The destination folder disappeared mid-export. | Thư mục đích biến mất trong lúc xuất. | batch row (`hint.chooseFolder`) | choose a folder, retry |
| `io.disk_full` | ExportError.DiskFull | The disk is full. | Ổ đĩa hết chỗ. | batch row (`hint.chooseFolder`) | choose another folder, retry |
| `io.encode` | IoError (encode leg) | Encoding the image failed. The export was not written. | Mã hóa ảnh thất bại. Chưa ghi tệp xuất. | toast, batch row | retry; report if persistent |
| `io.permission` | ExportError.PermissionDenied | Permission denied writing the output. | Không có quyền ghi file xuất. | batch row (`hint.checkPermissions`) | fix permissions, retry |
| `io.read` | IoError.Unreadable | The file could not be opened. It may have been moved, renamed, or you lack permission. | Không mở được tệp. Tệp có thể đã bị di chuyển, đổi tên, hoặc bạn thiếu quyền truy cập. | toast (`hint.fileMissing`) | re-open from Home |
| `io.unsupported` | IoError.Unsupported | This format or variant is not supported. | Định dạng hoặc biến thể này không được hỗ trợ. | toast, batch row (`hint.fileUnsupported`) | use a supported format |
| `io.write` | IoError (write leg) | Writing the file failed. | Ghi file thất bại. | toast | check disk/permissions |
| `lut.checksum` | LutError.ChecksumMismatch | The LUT failed its integrity check. | LUT không vượt qua kiểm tra toàn vẹn. | toast, import dialog | re-import the original file |
| `lut.encoding_mismatch` | LutError.EncodingMismatch | The LUT expects a different color encoding than the pipeline provides. | LUT yêu cầu một kiểu mã màu khác với kiểu mà đường ống cung cấp. | toast | fix the declared encodings |
| `lut.encoding_unimplemented` | LutError | This LUT declares an encoding this build does not implement yet. | LUT này khai báo một kiểu mã màu mà bản build chưa hỗ trợ. | import dialog row | use a supported encoding |
| `lut.encoding_unknown` | LutError.EncodingUnknown | Unknown encoding identifier. | Định danh mã màu không hợp lệ. | import dialog row | fix the encoding identifier |
| `lut.library_io` | LutError (library) | Reading the LUT library folder failed. | Đọc thư mục thư viện LUT thất bại. | toast | check library folder |
| `lut.malformed` | LutError.Malformed | The LUT file is not a valid .cube. Check the reported line. | Tệp LUT không phải .cube hợp lệ. Xem dòng được báo lỗi. | import dialog row | fix the .cube file |
| `lut.missing` | LutError.Missing | The LUT referenced by this recipe is missing from the library. Re-import the original file. | LUT mà công thức tham chiếu không còn trong thư viện. Hãy nhập lại tệp gốc. | toast, batch row (`hint.reimportLut`) | re-import the LUT |
| `lut.unsupported` | LutError (limits) | This LUT format or size is not supported. | Định dạng hoặc kích thước LUT này không được hỗ trợ. | import dialog row | use a supported .cube |
| `model.delete` | ModelError | Deleting the model failed. | Xóa mô hình thất bại. | model manager row | retry; check folder |
| `model.install` | ModelError | Installing the model failed. | Cài đặt mô hình thất bại. | model manager row | re-check the file |
| `model.manifest_parse` | ModelError (gate) | The model manifest is malformed. | Manifest mô hình sai định dạng. | model manager row | fix the manifest |
| `model.manifest_read` | ModelError (gate) | Reading the model manifest failed. | Đọc manifest mô hình thất bại. | model manager row | check the file exists |
| `param.invalid` | PipelineError (validation) | A parameter value is out of range. | Giá trị tham số ngoài phạm vi cho phép. | toast | fix the highlighted value |
| `preset.missing` | app-layer (presets) | That preset no longer exists — refresh the list. | Preset này không còn nữa — làm mới danh sách. | preset manager | refresh the list |
| `preset.name` | app-layer (presets) | That preset name is not allowed. | Tên preset đó không được chấp nhận. | preset manager | pick another name |
| `preset.save` | app-layer (presets) | Saving the preset failed. | Lưu preset thất bại. | preset manager | retry; check disk |
| `recipe.io` | sidecar I/O (ARCH §7) | Reading or writing the recipe sidecar failed. | Đọc/ghi sidecar công thức thất bại. | toast | retry; check disk |
| `recipe.version` | sidecar I/O (ARCH §7) | The recipe sidecar uses an unsupported version. | Sidecar công thức dùng phiên bản không được hỗ trợ. | toast | let the app migrate it (v0 → v1) |
| `render.max_dim_out_of_range` | PipelineError (validation) | The requested preview size is out of range (256–2560 px). | Kích thước bản xem trước yêu cầu nằm ngoài phạm vi cho phép (256–2560 px). | toast | only reachable via a malformed IPC call — the UI clamps by construction; report a bug if seen |
| `session.history` | app-layer (session) | Nothing to undo or redo. | Không còn gì để hoàn tác hoặc làm lại. | toast (harmless) | none (informational) |
| `session.none` | app-layer (session) | No image is open. | Chưa mở ảnh nào. | toast | open an image first |
| `settings.save` | app-layer (settings) | Saving the settings failed. | Lưu cài đặt thất bại. | toast | retry; check disk |
| `stock.missing` | PipelineError.StageFailed | A stock profile is missing from this build. | Thiếu hồ sơ phóng trong bản build này. | amber badge, batch row (`hint.restoreStock`) | reinstall profiles |

## Fuzz + battery coverage of the error producers

* Decoder rejections: `fuzz/decode_image` (typed `IoError` for every
  garbage class; zero crashes on the seed corpus).
* LUT parser rejections: `fuzz/cube_parser` + the 30-case unit corpus.
* Sidecar/manifest rejections: `fuzz/sidecar_json`, `fuzz/manifest_json`.
* Export crash-safety (the error states that matter mid-write):
  `crates/io/tests/export_kill.rs` SIGKILL battery — a killed export
  leaves either no file or a complete one, never a torn one.

## Honest gaps

* `ExportError::UnsupportedCombination{format,depth,alpha}` from §8 is
  prevented *structurally* at the UI layer (per-format factual controls —
  JPEG depth selector disabled, etc.) rather than mapped to a code; the
  CLI reports it as a plain string error. A dedicated code lands if/when
  an engine path can produce it after M4.
* `PipelineError::Cancelled` has no code — cancellation is a normal
  state surfaced as progress UI, not an error.
