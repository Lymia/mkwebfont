# Version 0.2.0-alpha5 (2024-07-07)

* Fix issue with the fallback font.

# Version 0.2.0-alpha4 (2024-07-07)

* Various bugfixes.
* Omit generating `@font-face` declarations for fonts not actually used on a particular webpage.
* Removed the experimental `adjacency` splitter.
* Implement automatic fallback font generation in webroot mode.
* Updated harfbuzz to 9.0.0

# Version 0.2.0-alpha3 (2024-07-01)

* Minor bugfixes.

# Version 0.2.0-alpha2 (2024-06-27)

* Added support for downloading fonts from Google Fonts by name.
* Added support for automatically generating webfonts for static websites, based on the CSS and HTML.
* While it isn't removed yet, the alpha1 feature to use common crawl data to subset will likely be removed eventually, if I can't improve the effectiveness. It currently still doesn't work well.

# Version 0.2.0-alpha1 (2024-05-19)

* Major revisions to the command line arguments. Should be more consistent now.
* Added a new feature to subset using data taken from Common Crawl rather than using Google's subsets. This seems to be much better for Chinese text, and not very much better for Japanese text. (TODO: Separate by CJK languages. Japanese and Chinese are the biggest culprits.)

# Version 0.1.1 (2024-04-08)

* Documentation changes.

# Version 0.1.0 (2024-04-08)

* Initial release.
