`lan.svg`, `mail.svg`, `man.svg` and `vf[FILL,GRAD,opsz,wght].ttf` are copies of files used for internal testing.

`mostly_off_curve.svg` was generated from `mostly_off_curve.ttf`, taken from `font-test-data`.

The svg files were generated using the [hb-draw](https://harfbuzz.github.io/harfbuzz-hb-draw.html) APIs
orchestrated by a small non-public C++ program.

`mat_symbols_subset.ttf` is a subset of Material Symbols containing a bunch of popular icons, cut using https://github.com/rsheeter/subset-gf-icons

   ```shell
   # clone https://github.com/google/material-design-icons
   # clone https://github.com/rsheeter/subset-gf-icons and install in a venv
   # CLI examples assume we are in the directory containing sleipnir, material-design-icons, subset-gf-icons

   $ subset_gf_icons material-design-icons/variablefont/MaterialSymbolsOutlined\[FILL\,GRAD\,opsz\,wght\].ttf check_box_outline_blank check_box person edit edit_off menu done close thumb_up thumb_down mic arrow_back home download info image mic_off share delete favorite lock photo_camera shopping_cart
   $ cp 'material-design-icons/variablefont/MaterialSymbolsOutlined[FILL,GRAD,opsz,wght]-subset.ttf' sleipnir/resources/testdata/MaterialSymbolsOutlinedVF-Popular.ttf
   ```