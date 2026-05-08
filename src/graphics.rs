use crate::glyph::Glyph;
const MAX_FILENAME_SIZE: usize = 256;
const MAX_INFO_LEN: usize = 256;
const MAX_IMAGE_RECTS: usize = 20;

/// The type used in this file to represent time. Used both for time differences
/// and absolute times (as milliseconds since an arbitrary point in time, see
/// `initialization_time`).
type Milliseconds = i64;

enum ScaleMode {
    SCALE_MODE_UNSET = 0,
    /// Stretch or shrink the image to fill the box, ignoring aspect ratio.
    SCALE_MODE_FILL = 1,
    /// Preserve aspect ratio and fit to width or to height so that the
    /// whole image is visible.
    SCALE_MODE_CONTAIN = 2,
    /// Do not scale. The image may be cropped if the box is too small.
    SCALE_MODE_NONE = 3,
    /// Do not scale, unless the box is too small, in which case the image
    /// will be shrunk like with `SCALE_MODE_CONTAIN`.
    SCALE_MODE_NONE_OR_CONTAIN = 4,
}

enum AnimationState {
    ANIMATION_STATE_UNSET = 0,
    /// The animation is stopped. Display the current frame, but don't
    /// advance to the next one.
    ANIMATION_STATE_STOPPED = 1,
    /// Run the animation to then end, then wait for the next frame.
    ANIMATION_STATE_LOADING = 2,
    /// Run the animation in a loop.
    ANIMATION_STATE_LOOPING = 3,
}

/// The status of an image. Each image uploaded to the terminal is cached on
/// disk, then it is loaded to ram when needed.
enum ImageStatus {
    STATUS_UNINITIALIZED = 0,
    STATUS_UPLOADING = 1,
    STATUS_UPLOADING_ERROR = 2,
    STATUS_UPLOADING_SUCCESS = 3,
    STATUS_RAM_LOADING_ERROR = 4,
    STATUS_RAM_LOADING_IN_PROGRESS = 5,
    STATUS_RAM_LOADING_SUCCESS = 6,
}

const image_status_strings: [&str; 6] = [
    "STATUS_UNINITIALIZED",
    "STATUS_UPLOADING",
    "STATUS_UPLOADING_ERROR",
    "STATUS_UPLOADING_SUCCESS",
    "STATUS_RAM_LOADING_ERROR",
    "STATUS_RAM_LOADING_SUCCESS",
];

enum ImageUploadingFailure {
    ERROR_OVER_SIZE_LIMIT = 1,
    ERROR_CANNOT_OPEN_CACHED_FILE = 2,
    ERROR_UNEXPECTED_SIZE = 3,
    ERROR_CANNOT_COPY_FILE = 4,
    ERROR_CANNOT_OPEN_SHM = 5,
}

const image_uploading_failure_strings: [&str; 6] = [
    "NO_ERROR",
    "ERROR_OVER_SIZE_LIMIT",
    "ERROR_CANNOT_OPEN_CACHED_FILE",
    "ERROR_UNEXPECTED_SIZE",
    "ERROR_CANNOT_COPY_FILE",
    "ERROR_CANNOT_OPEN_SHM",
];

////////////////////////////////////////////////////////////////////////////////
//
// We use the following structures to represent images and placements:
//
//   - Image: this is the main structure representing an image, usually created
//     by actions 'a=t', 'a=T`. Each image has an id (image id aka client id,
//     specified by 'i='). An image may have multiple frames (ImageFrame) and
//     placements (ImagePlacement).
//
//   - ImageFrame: represents a single frame of an image, usually created by
//     the action 'a=f' (and the first frame is created with the image itself).
//     Each frame has an index and also:
//     - a file containing the frame data (considered to be "on disk", although
//       it's probably in tmpfs),
//     - an imlib object containing the fully composed frame (i.e. the frame
//       data from the file composed onto the background frame or color). It is
//       not ready for display yet, because it needs to be scaled and uploaded
//       to the X server.
//
//   - ImagePlacement: represents a placement of an image, created by 'a=p' and
//     'a=T'. Each placement has an id (placement id, specified by 'p='). Also
//     each placement has an array of pixmaps: one for each frame of the image.
//     Each pixmap is a scaled and uploaded image ready to be displayed.
//
// Images are store in the `images` hash table, mapping image ids to Image
// objects (allocated on the heap).
//
// Placements are stored in the `placements` hash table of each Image object,
// mapping placement ids to ImagePlacement objects (also allocated on the heap).
//
// ImageFrames are stored in the `first_frame` field and in the
// `frames_beyond_the_first` array of each Image object. They are stored by
// value, so ImageFrame pointer may be invalidated when frames are
// added/deleted, be careful.
//
////////////////////////////////////////////////////////////////////////////////

// KHASH_MAP_INIT_INT(id2image, struct Image *)
// KHASH_MAP_INIT_INT(id2placement, struct ImagePlacement *)

pub struct ImageFrame {
    /// The image this frame belongs to.
    image: *const Image,
    /// The 1-based index of the frame. Zero if the frame isn't initialized.
    index: usize,
    /// The last time when the frame was displayed or otherwise touched.
    atime: Milliseconds,
    /// The background color of the frame in the 0xRRGGBBAA format.
    background_color: u32,
    /// The index of the background frame. Zero to use the color instead.
    background_frame_index: usize,
    /// The duration of the frame in milliseconds.
    gap: Milliseconds,
    /// The expected size of the frame image file (specified with 'S='),
    /// used to check if uploading succeeded.
    expected_size: usize,
    /// Format specification (see the `f=` key).
    format: usize,
    /// Pixel width and height of the non-composed (on-disk) frame data. May
    /// differ from the image (i.e. first frame) dimensions.
    data_pix_width: usize,
    data_pix_height: usize,
    /// The offset of the frame relative to the first frame.
    x: usize,
    y: usize,
    /// Compression mode (see the `o=` key).
    compression: u8,
    /// The status (see `ImageStatus`).
    status: u8,
    /// The reason of uploading failure (see `ImageUploadingFailure`).
    uploading_failure: u8,
    /// Whether failures and successes should be reported ('q=').
    quiet: bool,
    /// Whether to blend the frame with the background or replace it.
    blend: bool,
    /// The file corresponding to the on-disk cache, used when uploading.
    // FILE *open_file;
    open_file: Option<std::fs::File>,

    /// The size of the corresponding file cached on disk.
    disk_size: usize,
    /// The imlib object containing the fully composed frame. It's not
    /// scaled for screen display yet.
    // TODO:: use imlib2
    imlib_object: Option<()>,
    // Imlib_Image imlib_object;
}

pub struct Image {
    /// The client id (the one specified with 'i='). Must be nonzero.
    image_id: usize,
    /// The client id specified in the query command (`a=q`). This one must
    /// be used to create the response if it's non-zero.
    query_id: usize,
    /// The number specified in the transmission command (`I=`). If
    /// non-zero, it may be used to identify the image instead of the
    /// image_id, and it also should be mentioned in responses.
    image_number: usize,
    /// The last time when the image was displayed or otherwise touched.
    atime: Milliseconds,
    /// The total duration of the animation in milliseconds.
    total_duration: usize,
    /// The total size of cached image files for all frames.
    total_disk_size: usize,
    /// The global index of the creation command. Used to decide which image
    /// is newer if they have the same image number.
    gloabl_command_index: usize,
    /// The 1-based index of the currently displayed frame.
    current_frame: usize,
    /// The state of the animation, see `AnimationState`.
    animation_state: u8,
    /// The absolute time that is assumed to be the start of the current
    /// frame (in ms since initialization).
    current_frame_time: Milliseconds,
    /// The absolute time of the last redraw (in ms since initialization).
    /// Used to check whether it's the first time we draw the image in the
    /// current redraw cycle.
    last_redraw: Milliseconds,
    /// The absolute time of the next redraw (in ms since initialization).
    /// 0 means no redraw is scheduled.
    next_redraw: Milliseconds,
    /// The unscaled pixel width and height of the image. Usually inherited
    /// from the first frame.
    pix_width: usize,
    pix_height: usize,
    /// The first frame.
    first_frame: ImageFrame,

    /// The array of frames beyond the first one.
    // kvec_t(ImageFrame) frames_beyond_the_first;
    frames_beyond_the_first: (),
    /// Image placements.
    // khash_t(id2placement) * placements;
    placements: (),
    /// The default placement.
    default_placement: u32,
    /// The initial placement id, specified with the transmission command,
    /// used to report success or failure.
    initial_placement_id: u32,
}

pub struct ImagePlacement {
    /// The image this placement belongs to.
    // Image *image;
    image: *const Image,
    /// The id of the placement. Must be nonzero.
    placement_id: u32,
    /// The last time when the placement was displayed or otherwise touched.
    atime: Milliseconds,
    /// The 1-based index of the protected pixmap. We protect a pixmap in
    /// gr_load_pixmap to avoid unloading it right after it was loaded.
    protected_frame: i32,
    /// Whether the placement is used only for Unicode placeholders.
    virtual_: bool,
    /// The scaling mode (see `ScaleMode`).
    scale_mode: u8,
    /// Height and width in cells.
    rows: u16,
    cols: u16,
    /// Top-left corner of the source rectangle ('x=' and 'y=').
    src_pix_x: u32,
    src_pix_y: u32,
    /// Height and width of the source rectangle (zero if full image).
    src_pix_width: u32,
    src_pix_height: u32,
    /// The image appropriately scaled and uploaded to the X server. This
    /// pixmap is premultiplied by alpha.
    // Pixmap first_pixmap,
    first_pixmap: (),
    /// The array of pixmaps beyond the first one.
    // kvec_t(Pixmap) pixmaps_beyond_the_first,
    pixmaps_beyond_the_first: (),
    /// The dimensions of the cell used to scale the image. If cell
    /// dimensions are changed (font change), the image will be rescaled.
    scaled_cw: u16,
    scaled_ch: u16,
    /// If true, do not move the cursor when displaying this placement
    /// (non-virtual placements only).
    do_not_move_cursor: bool,
    /// The text underneath this placement, valid only for classic
    /// placements. On deletion, the text is restored. This is a malloced
    /// array of rows*cols Glyphs.
    text_underneath: Box<[Glyph]>,
}

/// A rectangular piece of an image to be drawn.
struct ImageRect {
    image_id: u32,
    placement_id: u32,
    /// The position of the rectangle in pixels.
    screen_x_pix: usize,
    screen_y_pix: usize,
    /// The starting row on the screen.
    screen_y_row: usize,
    /// The part of the whole image to be drawn, in cells. Starts are
    /// zero-based, ends are exclusive.
    img_start_col: usize,
    img_end_col: usize,
    img_start_row: usize,
    img_end_row: usize,
    /// The current cell width and height in pixels.
    cw: usize,
    ch: usize,
    /// Whether colors should be inverted.
    reverse: bool,
}

/// Executes `code` for each frame of an image. Example:
///
///     foreach_frame(image, frame, {
///         printf("Frame %d\n", frame->index);
///     });
///

pub fn foreach_frame(image: &Image, mut code: impl FnMut(&ImageFrame)) {
    todo!()
}

/// Executes `code` for each pixmap of a placement. Example:
///
///     foreach_pixmap(placement, pixmap, {
///         ...
///     });
///
pub fn foreach_pixmap(placement: &ImagePlacement, pixmapvar: (), mut code: impl FnMut(())) {
    todo!()
}

pub fn gr_find_image(image_id: usize) -> &'static Image {
    todo!()
}
