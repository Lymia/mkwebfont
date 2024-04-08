# mkwebfont

mkwebfont is a simple tool for turning .ttf/.otf files into webfonts for self-hosting, without the complication or lack
of flexibility that prepackaged webfonts or hosted webfonts have. It's designed to be an easy one-command solution
that doesn't require complicated scripts or specific understanding of .woff2 or fonts to make work.

Like Google Fonts, it splits the fonts into subsets that allows only part of the font to be loaded as needed,
usually based on the languages used.

## Usage

To install it, simply run the following command:
```bash
cargo install mkwebfont
```

Then, run the following command to create a webfont:
```bash
# Assumes that you serve `https://example.com/` from a directory of `/srv/http/root`.
# Adjust these paths for your use case
mkdir -p /srv/http/webroot/static/webfonts
mkwebfont \
    --store /srv/http/webroot/static/webfonts --store-uri "https://example.com/static/webfonts/" \
    -o /srv/http/webroot/static/fonts.css fonts/* 
```

After this, you can simply include the stylesheet at `https://example.com/static/fonts.css`, and then you can use any
fonts that you included in the command line (e.g. with the `fonts/*` in the example) in your website.

## License

This project is licensed under the Apache License Version 2.0.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in mkwebfont by you, as
defined in the Apache-2.0 license, shall be licensed as above, without any additional terms or conditions.
