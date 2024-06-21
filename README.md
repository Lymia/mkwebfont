# mkwebfont

mkwebfont is a simple tool for turning .ttf/.otf files into webfonts for self-hosting, without the complication or lack of flexibility that prepackaged webfonts or hosted webfonts have. It's designed to be an easy one-command solution that doesn't require complicated scripts or specific understanding of .woff2 or fonts to make work.

Like Google Fonts, it splits the fonts into subsets that allows only part of the font to be loaded as needed, usually based on the languages used.

## Usage

### Installation

To install it, simply run the following command:

```bash
cargo +nightly install mkwebfont
```

Alternatively, download an AppImage from the [releases page](https://github.com/Lymia/mkwebfont/releases).

### Basic Usage

Run the following command to create a basic webfont:

```bash
# Assumes that you serve `https://example.com/` from a directory of `/srv/http/root`.
# Adjust these paths for your use case
mkdir -p /srv/http/webroot/static/webfonts
mkwebfont \
    --store /srv/http/webroot/static/webfonts --store-uri "https://example.com/static/webfonts/" \
    -o /srv/http/webroot/static/fonts.css fonts/* 
```

After this, you can simply include the stylesheet at `https://example.com/static/fonts.css`, and then you can use any fonts that you included in the command line (e.g. with the `fonts/*` in the example) in your website.

### Usage for Static Websites

mkwebfont has special support for *completely static* websites that are use only HTML and CSS with no dynamic features. For these websites, it can automatically create webfonts that contain only glyphs actually used in the website and automatically download any fonts requested in the CSS (and available on Google Fonts) from the internet.

Run the following command to create webfonts for a static website:

```bash
# Assumes that you serve `https://example.com/` from a directory of `/srv/http/root`.
# All fonts must be in `custom_fonts/` or available on Google Fonts (in which case they will be automatically downloaded).
# Adjust these paths for your use case
mkdir -p /srv/http/webroot/static/webfonts
mkwebfont \
    --store /srv/http/webroot/static/webfonts --store-uri "https://example.com/static/webfonts/" \
    -o /srv/http/webroot/static/fonts.css --webroot /srv/http/webroot/static/ custom_fonts/*
```

You may specify a special font called `"mkwebfont Fallback"` as the last font in your font stacks in this mode. This creates a webfont based on Noto Sans variants that can render any current Unicode character. 

## License

This project is licensed under the Apache License Version 2.0.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in mkwebfont by you, as defined in the Apache-2.0 license, shall be licensed as above, without any additional terms or conditions.
