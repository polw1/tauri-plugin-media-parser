# markdownlint-rule-indent-alignment

<!-- markdownlint-disable line-length -->
[![NPM Version](https://img.shields.io/npm/v/@silvermine/markdownlint-rule-indent-alignment.svg)](https://www.npmjs.com/package/@silvermine/markdownlint-rule-indent-alignment)
[![License](https://img.shields.io/github/license/silvermine/markdownlint-rule-indent-alignment.svg)](./LICENSE)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://conventionalcommits.org)
<!-- markdownlint-enable line-length -->

## What?

This is a custom [markdownlint](https://github.com/DavidAnson/markdownlint) rule to lint
the alignment of list items and blocks (e.g. code blocks). For example:

| Invalid :no_entry:                                                                           | Valid :white_check_mark:                                                               |
|----------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------|
| <pre>Lorem ipsum dolor sit amet<br>   consectetur adipiscing elit.</pre>                     | <pre>Lorem ipsum dolor sit amet<br>consectetur adipiscing elit.</pre>                  |
| <pre>\* Lorem ipsum<br> * Dolor sit amet</pre>                                               | <pre>* Lorem ipsum<br>  * Dolor sit amet</pre>                                         |
| <pre>1. Lorem ipsum<br>  * Dolor sit amet</pre>                                              | <pre>1. Lorem ipsum<br>   * Dolor sit amet</pre>                                       |
| <pre>* Lorem ipsum dolor sit amet<br>consectetur adipiscing elit</pre>                       | <pre>* Lorem ipsum dolor sit amet<br>  consectetur adipiscing elit</pre>               |
| <pre>1. Lorem ipsum dolor sit amet<br>     consectetur adipiscing elit</pre>                 | <pre>1. Lorem ipsum dolor sit amet<br>   consectetur adipiscing elit</pre>             |
| <pre>* Lorem:<br><br>    &#96;&#96;&#96;text<br>    hello world<br>    &#96;&#96;&#96;</pre> | <pre>* Lorem:<br><br>  &#96;&#96;&#96;text<br>  hello world<br>  &#96;&#96;&#96;</pre> |
| <pre>> Lorem ipsum dolor sit amet<br> > consectetur adipiscing elit</pre>                    | <pre>> Lorem ipsum dolor sit amet<br>> consectetur adipiscing elit</pre>               |

## Usage

To use this custom markdownlint rule two things needed:

   1. Update your [markdownlint config][markdownlint-config] to include
      `"indent-alignment": true`
   2. Add this rule to the list of [custom markdownlint rules][custom-rules-config].
      If using [markdownlint-cli](https://github.com/igorshubovych/markdownlint-cli), this
      would look something like:

      <!-- markdownlint-disable line-length -->
      ```bash
      markdownlint -r './node_modules/@silvermine/markdownlint-rule-indent-alignment/src/indent-alignment.js' -c .markdownlint.json README.md
      ```
      <!-- markdownlint-enable line-length -->

## Rule configuration

This rule has the following config options:

   * `ul_indent`: Desired indention of `ul` lists (`number`, default `undefined`)
      * By default, child `ul` items will be aligned with the content. When `ul_indent` is
        set, the rule will replicate the behavior of the [`ul-indent` (MD007)
        rule][md007].
      * Examples:
         * `ul_indent: undefined`

           ```text
           * top-level item
             * sub-item
           ```

         * `ul_indent: 3`

           ```text
           * top-level item
              * sub-item
           ```

   * `start_indent`: Desired indention of all top-level lists (`number`, default
     `undefined`)
      * By default, top-level lists are allowed to be indented at any level as long as
        it's consistent. When `start_indent` is set, then any top-level list must be
        indented at that level.
      * Examples:
         * `start_indent: 3`

           ```text
              * top-level item
                * sub-item

              1. top-level item
                 1. sub-item
           ```

[markdownlint-config]: https://github.com/DavidAnson/markdownlint#optionsconfig
[custom-rules-config]: https://github.com/DavidAnson/markdownlint#optionscustomrules
[md007]: https://github.com/DavidAnson/markdownlint/blob/main/doc/md007.md

## License

This software is released under the MIT license. See [the license file](LICENSE) for more
details.
