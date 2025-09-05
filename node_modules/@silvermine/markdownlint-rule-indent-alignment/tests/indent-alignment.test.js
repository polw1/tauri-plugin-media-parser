'use strict';

const { expect } = require('chai');

const { markdownlint } = require('markdownlint').promises,
      indentAlignment = require('../src/indent-alignment');

describe('Indent Alignment', () => {

   async function runTest(lines, ruleConfig = true) {
      const text = lines.join('\n') + '\n';

      return markdownlint({
         config: {
            default: false,
            'indent-alignment': ruleConfig,
         },
         customRules: [ indentAlignment ],
         strings: {
            text,
         },
      });
   }

   async function testValidExample(lines, ruleConfig) {
      expect((await runTest(lines, ruleConfig)).text).to.eql([]);
   }

   async function testInvalidExample(lines, ruleConfig) {
      const results = await runTest(lines, ruleConfig);

      expect(results.text).to.have.length.greaterThanOrEqual(1);
   }

   it('reports no errors in the grab bag test', async () => {
      await testValidExample([
         'top-level text',
         'wrapped top-level text',
         '',
         '* first-level list item',
         '  first-level wrapped line',
         '  1. second-level list item (two space)',
         '     second-level wrapped line',
         '',
         'top-level text',
         'wrapped top-level text',
         '',
         '> * first-level list item',
         '>   first-level list item',
         '>   1. second-level list item (two space)',
         '>      second-level wrapped line',
         '',
         'top-level text',
         'wrapped top-level text',
         '',
         '    top-level four-space code block',
         '    wrapped top-level four-space code block',
         '',
         '* first-level list item',
         '  first-level wrapped line',
         '',
         '  > blockquote',
         '',
         '  another first-level paragraph',
         '  another first-level wrapped line',
         '',
         '  > blockquote',
         '',
         '  ```',
         '  code block',
         '  ```',
         '',
         '  more text',
         '',
         '      indented code block',
         '',
         '  more text',
         '',
         'Are not footnotes[^1] cool?',
         '',
         '[^1]: first footnote paragraph',
         '      first paragraph wrapped line',
         '',
         '      second paragraph line',
         '',
         '> * first-level list item',
         '>   first-level list item',
         '>   1. second-level list item (two space)',
         '>      second-level wrapped line',
         '>',
         '>> * first-level list item',
         '>>   first-level list item',
         '>>   1. second-level list item (two space)',
         '>>      second-level wrapped line',
      ]);
      await testValidExample([
         'top-level text',
         'wrapped top-level text',
         '',
         '* first-level list item',
         '  first-level wrapped line',
         '   1. second-level list item (three space)',
         '      second-level wrapped line',
         '',
         'top-level text',
         'wrapped top-level text',
         '',
         '> * first-level list item',
         '>   first-level list item',
         '>    1. second-level list item (three space)',
         '>       second-level wrapped line',
         '',
         'top-level text',
         'wrapped top-level text',
         '',
         '    top-level four-space code block',
         '    wrapped top-level four-space code block',
         '',
         '* first-level list item',
         '  first-level wrapped line',
         '',
         '  > blockquote',
         '',
         '  another first-level paragraph',
         '  another first-level wrapped line',
         '',
         '  > blockquote',
         '',
         '  ```',
         '  code block',
         '  ```',
         '',
         '  more text',
         '',
         'Are not footnotes[^1] cool?',
         '',
         '[^1]: first footnote paragraph',
         '      first paragraph wrapped line',
         '',
         '      second paragraph line',
         '',
         '> * first-level list item',
         '>   first-level list item',
         '>    1. second-level list item (three space)',
         '>       second-level wrapped line',
         '>',
         '>> * first-level list item',
         '>>   first-level list item',
         '>>    1. second-level list item (three space)',
         '>>       second-level wrapped line',
         '',
         '## test page',
         '',
         '   1. contact information',
         '      1. contact information',
         '         * contact information',
         '            * Includes body content and a [**Contact Information Chooser**](#TODO) test',
         '               * contact information',
      ], { 'ul_indent': 3 });
   });

   it('reports no errors for correctly aligned paragraph lines', async () => {
      await testValidExample([
         'line one',
         'line two',
      ]);
      await testValidExample([
         '* first-level list item',
         '  first-level wrapped line',
      ]);
      await testValidExample([
         '1. first-level list item',
         '   first-level wrapped line',
      ]);
      await testValidExample([
         '* first-level list item',
         '  * second-level list item (two space)',
         '    second-level wrapped line',
      ]);
      await testValidExample([
         '* first-level list item',
         '   * second-level list item (three space)',
         '     second-level wrapped line',
      ], { 'ul_indent': 3 });
      await testValidExample([
         '1. first-level list item',
         '   1. second-level list item',
         '      second-level wrapped line',
      ]);
      await testValidExample([
         '1. first-level list item',
         '   * second-level list item',
         '     second-level wrapped line',
      ]);
      await testValidExample([
         '* first-level list item',
         '  1. second-level list item (two space)',
         '     second-level wrapped line',
      ]);
      await testValidExample([
         '* first-level list item',
         '   1. second-level list item (three space)',
         '      second-level wrapped line',
      ], { 'ul_indent': 3 });
   });

   it('reports errors for incorrectly aligned paragraph lines', async () => {
      await testInvalidExample([
         'line one',
         ' line two (+1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         ' first-level wrapped line (-1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '   first-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '   first-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '  first-level wrapped line (-1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '    first-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '   * second-level list item',
         '    second-level wrapped line (-1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '   * second-level list item',
         '      second-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '   1. second-level list item',
         '     second-level wrapped line (-1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '   1. second-level list item',
         '       second-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '   * second-level list item',
         '    second-level wrapped line (-1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '   * second-level list item',
         '      second-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '  1. second-level list item (two space)',
         '    second-level wrapped line (-1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '  1. second-level list item (two space)',
         '      second-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '   1. second-level list item (three space)',
         '     second-level wrapped line (-1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '   1. second-level list item (three space)',
         '       second-level wrapped line (+1 offset)',
      ]);
      await testInvalidExample([
         '[^1]: first line',
         '       second line (+1 offset)',
         '[^2]: first line',
         '     second line (-1 offset)',
      ]);
   });

   it('reports errors for indented top-level paragraphs', async () => {
      await testInvalidExample([
         ' one space paragraph',
         ' line two',
      ]);
      await testInvalidExample([
         ' one space paragraph',
         'line two (-1 offset)',
      ]);
      await testInvalidExample([
         ' one space paragraph',
         '  line two (+1 offset)',
      ]);
      await testInvalidExample([
         '  two space paragraph',
         '  line two',
      ]);
      await testInvalidExample([
         '  two space paragraph',
         'line two (-2 offset)',
      ]);
      await testInvalidExample([
         '  two space paragraph',
         ' line two (-1 offset)',
      ]);
      await testInvalidExample([
         '  two space paragraph',
         ' line two (+1 offset)',
      ]);
      await testInvalidExample([
         '   three space paragraph',
         '   line two',
      ]);
      await testInvalidExample([
         '   three space paragraph',
         'line two (-3 offset)',
      ]);
      await testInvalidExample([
         '   three space paragraph',
         ' line two (-2 offset)',
      ]);
      await testInvalidExample([
         '   three space paragraph',
         '  line two (-1 offset)',
      ]);
   });

   it('reports no errors for correctly aligned blocks', async () => {
      await testValidExample([
         '* first list item',
         '* second list item',
      ]);
      await testValidExample([
         '* first-level list item',
         '  * second-level list item (two space)',
      ]);
      await testValidExample([
         '* first-level list item',
         '   * second-level list item (three space)',
      ], { 'ul_indent': 3 });
      await testValidExample([
         '1. first list item',
         '1. second list item',
      ]);
      await testValidExample([
         '1. first-level list item',
         '   1. second-level list item',
      ]);
      await testValidExample([
         '1. first-level list item',
         '   * second-level list item',
      ]);
      await testValidExample([
         '* first-level list item',
         '  1. second-level list item (two space)',
      ]);
      await testValidExample([
         '* first-level list item',
         '   1. second-level list item (three space)',
      ], { 'ul_indent': 3 });
      await testValidExample([
         '* first-level list item',
         '',
         '  > blockquote',
      ]);
      await testValidExample([
         '1. first-level list item',
         '',
         '   > blockquote',
      ]);
      await testValidExample([
         '> * first list item',
         '> * second list item',
      ]);
      await testValidExample([
         '> * first-level list item',
         '>   * second-level list item (two space)',
      ]);
      await testValidExample([
         '> * first-level list item',
         '>    * second-level list item (three space)',
      ], { 'ul_indent': 3 });
      await testValidExample([
         '> 1. first list item',
         '> 1. second list item',
      ]);
      await testValidExample([
         '> 1. first-level list item',
         '>    1. second-level list item',
      ]);
      await testValidExample([
         '> 1. first-level list item',
         '>    * second-level list item',
      ]);
      await testValidExample([
         '> * first-level list item',
         '>   1. second-level list item (two space)',
      ]);
      await testValidExample([
         '> * first-level list item',
         '>    1. second-level list item (three space)',
      ], { 'ul_indent': 3 });
      await testValidExample([
         '> * first-level list item',
         '>',
         '>   > blockquote',
      ]);
      await testValidExample([
         '> 1. first-level list item',
         '>',
         '>    > blockquote',
      ]);
   });

   it('reports errors for incorrectly aligned blocks', async () => {
      await testInvalidExample([
         '* first-level list item',
         ' * second-level list item (-1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '   * second-level list item (+1 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         '    * second-level list item (+2 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item (-3 offset)',
         '   * second-level list item',
      ], { 'ul_indent': 3, 'start_indent': 3 });
      await testInvalidExample([
         '1. first-level list item (-3 offset)',
         '   * second-level list item',
      ], { 'start_indent': 3 });
      await testInvalidExample([
         '* first-level list item',
         ' * second-level list item (-1 offset)',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '* first-level list item',
         '  * second-level list item (0 offset)',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '* first-level list item',
         '    * second-level list item (+2 offset)',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '1. first-level list item',
         '  1. second-level list item (-1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '    1. second-level list item (+1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '  1. second-level list item (-1 offset)',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '1. first-level list item',
         '    1. second-level list item (+1 offset)',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '1. first-level list item',
         '  * second-level list item (-1 offset)',
         '1. first-level list item',
         '    * second-level list item (+1 offset)',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '1. first-level list item',
         '  * second-level list item (-1 offset)',
         '1. first-level list item',
         '    * second-level list item (+1 offset)',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '* first-level list item',
         ' 1. second-level list item (-1 offset)',
         '* first-level list item',
         '   1. second-level list item (+1 offset)',
         '* first-level list item',
         '    1. second-level list item (+2 offset)',
      ]);
      await testInvalidExample([
         '* first-level list item',
         ' 1. second-level list item (-1 offset)',
         '* first-level list item',
         '  1. second-level list item (0 offset)',
         '* first-level list item',
         '    1. second-level list item (+2 offset)',
      ], { 'ul_indent': 3 });
   });

   it('reports no errors for left-aligned paragraphs', async () => {
      await testValidExample([
         'Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
         'Pellentesque vestibulum lectus non tellus congue,',
         'eu ultricies metus ultrices.',
         '',
         'Curabitur ut posuere est, quis convallis orci.',
         'Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
   });

   it('reports errors for non-left-aligned paragraphs', async () => {
      await testInvalidExample([
         '   Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
         'Pellentesque vestibulum lectus non tellus congue,',
         'eu ultricies metus ultrices.',
         '',
         '   Curabitur ut posuere est, quis convallis orci.',
         'Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
      await testInvalidExample([
         'Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
         '   Pellentesque vestibulum lectus non tellus congue,',
         '   eu ultricies metus ultrices.',
         '',
         'Curabitur ut posuere est, quis convallis orci.',
         ' Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
   });

   it('reports no errors for left-aligned list items', async () => {
      await testValidExample([
         '* ul item',
         '  * ul item',
         '    1. ol item',
         '       * ul item',
         '    2. ol item',
      ]);
      await testValidExample([
         '* first list item',
         '* second list item',
         '   * second-level list item',
         '      * third-level list item',
         '* first list item',
         '  wrapped line for the first list item',
         '* second list item',
         '  wrapped line for the second list item',
         '   * second-level list item',
         '     wrapped line for the second-level list item',
         '      * third-level list item',
         '        wrapped line for the third-level list item',
      ], { 'ul_indent': 3, 'start_indent': 0 });
   });

   it('reports errors for non-left-aligned list items', async () => {
      await testInvalidExample([
         '* first list item',
         '* second list item',
         '   * second-level list item',
         '      * third-level list item',
         '* first list item',
         'wrapped line for the first list item',
         '* second list item',
         'wrapped line for the second list item',
         '   * second-level list item',
         '   wrapped line for the second-level list item',
         '      * third-level list item',
         '      wrapped line for the third-level list item',
      ], { 'ul_indent': 3 });
      await testInvalidExample([
         '* first list item',
         '* second list item',
         '   * second-level list item',
         '      * third-level list item',
         '* first list item',
         '    wrapped line for the first list item',
         '* second list item',
         '    wrapped line for the second list item',
         '   * second-level list item',
         '       wrapped line for the second-level list item',
         '      * third-level list item',
         '          wrapped line for the third-level list item',
      ], { 'ul_indent': 3 });
   });

   it('reports no errors for properly aligned lists with formatting', async () => {
      await testValidExample([
         '* `code list item`',
         '* list item ending with `code`',
         '* `code` starting list item',
         '* _italic list item_',
         '* list item ending with _italic_',
         '* _italic_ starting list item',
         '* **bold list item**',
         '* list item ending with **bold**',
         '* **bold** starting list item',
         '* placeholder text',
         '  `code wrapped item`',
         '* placeholder text',
         '  wrapped item ending with `code`',
         '* placeholder text',
         '  `code` starting wrapped item',
         '* placeholder text',
         '  _italic wrapped item_',
         '* placeholder text',
         '  wrapped item ending with _italic_',
         '* placeholder text',
         '  _italic_ starting wrapped item',
         '* placeholder text',
         '  **bold wrapped item**',
         '* placeholder text',
         '  wrapped item ending with **bold**',
         '* placeholder text',
         '  `code` starting wrapped item',
         '* placeholder text',
         '  **bold** starting wrapped item',
         '* placeholder text',
         '   * placeholder text',
         '     `code wrapped item`',
         '   * placeholder text',
         '     wrapped item ending with `code`',
         '   * placeholder text',
         '     `code` starting wrapped item',
         '   * placeholder text',
         '     _italic wrapped item_',
         '   * placeholder text',
         '     wrapped item ending with _italic_',
         '   * placeholder text',
         '     _italic_ starting wrapped item',
         '   * placeholder text',
         '     **bold wrapped item**',
         '   * placeholder text',
         '     wrapped item ending with **bold**',
         '   * placeholder text',
         '     **bold** starting wrapped item',
      ], { 'ul_indent': 3, 'start_indent': 0 });
   });

   it('reports errors for improperly aligned lists with inline_code', async () => {
      await testInvalidExample([
         '* `code list item`',
         '* list item ending with `code`',
         '* `code` starting list item',
         '* placeholder text',
         ' `code wrapped item`',
         '* placeholder text',
         ' wrapped item ending with `code`',
         '* placeholder text',
         ' `code` starting wrapped item',
         '* placeholder text',
         ' `code` starting wrapped item',
         '* placeholder text',
         '   `code wrapped item`',
         '* placeholder text',
         '   wrapped item ending with `code`',
         '* placeholder text',
         '   `code` starting wrapped item',
         '* placeholder text',
         '   `code` starting wrapped item',
         '* placeholder text',
         '   * placeholder text',
         '    `code wrapped item`',
         '   * placeholder text',
         '    wrapped item ending with `code`',
         '   * placeholder text',
         '    `code` starting wrapped item',
         '   * placeholder text',
         '      `code wrapped item`',
         '   * placeholder text',
         '      wrapped item ending with `code`',
         '   * placeholder text',
         '      `code` starting wrapped item',
      ], { 'ul_indent': 3, 'start_indent': 3 });
   });

   it('reports errors for improperly aligned lists with bold text', async () => {
      await testInvalidExample([
         '* **bold list item**',
         '* list item ending with **bold**',
         '* **bold** starting list item',
         '* placeholder text',
         ' **bold wrapped item**',
         '* placeholder text',
         ' wrapped item ending with **bold**',
         '* placeholder text',
         ' **bold** starting wrapped item',
         '* placeholder text',
         ' **bold** starting wrapped item',
         '* placeholder text',
         '   **bold wrapped item**',
         '* placeholder text',
         '   wrapped item ending with **bold**',
         '* placeholder text',
         '   **bold** starting wrapped item',
         '* placeholder text',
         '   **bold** starting wrapped item',
         '* placeholder text',
         '   * placeholder text',
         '    **bold wrapped item**',
         '   * placeholder text',
         '    wrapped item ending with **bold**',
         '   * placeholder text',
         '    **bold** starting wrapped item',
         '   * placeholder text',
         '      **bold wrapped item**',
         '   * placeholder text',
         '      wrapped item ending with **bold**',
         '   * placeholder text',
         '      **bold** starting wrapped item',
      ], { 'ul_indent': 3, 'start_indent': 3 });
   });

   it('reports errors for improperly aligned lists with italicized text', async () => {
      await testInvalidExample([
         '* _italic list item_',
         '* list item ending with _italic_',
         '* _italic_ starting list item',
         '* placeholder text',
         ' _italic wrapped item_',
         '* placeholder text',
         ' wrapped item ending with _italic_',
         '* placeholder text',
         ' _italic_ starting wrapped item',
         '* placeholder text',
         ' _italic_ starting wrapped item',
         '* placeholder text',
         '   _italic wrapped item_',
         '* placeholder text',
         '   wrapped item ending with _italic_',
         '* placeholder text',
         '   _italic_ starting wrapped item',
         '* placeholder text',
         '   _italic_ starting wrapped item',
         '* placeholder text',
         '   * placeholder text',
         '    _italic wrapped item_',
         '   * placeholder text',
         '    wrapped item ending with _italic_',
         '   * placeholder text',
         '    _italic_ starting wrapped item',
         '   * placeholder text',
         '      _italic wrapped item_',
         '   * placeholder text',
         '      wrapped item ending with _italic_',
         '   * placeholder text',
         '      _italic_ starting wrapped item',
      ], { 'ul_indent': 3, 'start_indent': 3 });
   });

   it('reports no errors for indented lists', async () => {
      await testValidExample([
         '   * first list item',
         '   * second list item',
      ]);
      await testValidExample([
         '   * first list item',
         '     * sub-list item',
      ]);
      await testValidExample([
         '   * first list item',
         '      * sub-list item',
      ], { 'ul_indent': 3, 'start_indent': 3 });
      await testValidExample([
         '   1. first list item',
         '   1. second list item',
      ]);
      await testValidExample([
         '   1. first list item',
         '      1. sub-list item',
      ], { 'start_indent': 3 });
   });

   it('reports no errors for properly aligned code blocks', async () => {
      await testValidExample([
         'This is an indented code block:',
         '',
         '    console.log("Hello world");',
      ]);
      await testValidExample([
         'This is an indented code block that has code has two spaces to start the code:',
         '',
         '      console.log("Hello world");',
      ]);
      await testValidExample([
         'This is a fenced code block:',
         '',
         '```',
         'console.log("Hello world");',
         '```',
      ]);
      await testValidExample([
         '  * This is an indented code block in a list:',
         '',
         '        console.log("Hello world");',
      ]);
      await testValidExample([
         '  * This is an indented code block in a list that has code has two spaces to start the code:',
         '',
         '          console.log("Hello world");',
      ]);
      await testValidExample([
         '  * This is a fenced code block in a list:',
         '',
         '    ```',
         '    console.log("Hello world");',
         '    ```',
      ]);
      await testValidExample([
         'This is a fenced code block that is closed by the document:',
         '',
         '```',
         '  console.log("Hello world");',
      ]);
      await testValidExample([
         '  * This is a fenced code block in a list that is closed by the document:',
         '',
         '    ```',
         '    config:',
         '       key: value',
      ]);
   });

   it('reports errors for improperly aligned code blocks', async () => {
      await testInvalidExample([
         'This is an indented code block:',
         '',
         '  console.log("Hello world");',
      ]);
      await testInvalidExample([
         'This is a fenced code block:',
         '',
         '  ```',
         '  console.log("Hello world");',
         '  ```',
      ]);
      await testInvalidExample([
         '  * This is an indented code block in a list:',
         '',
         '      console.log("Hello world");',
      ]);
      await testInvalidExample([
         '  * This is a fenced code block in a list:',
         '',
         '  ```',
         '  console.log("Hello world");',
         '  ```',
      ]);
      await testInvalidExample([
         '  * This is a fenced code block in a list:',
         '',
         '      ```',
         '      console.log("Hello world");',
         '      ```',
      ]);
   });

   it('reports no errors for properly aligned links', async () => {
      await testValidExample([
         'Lorem ipsum dolor sit amet',
         '[consectetur adipiscing elit](https://example.com).',
      ]);
      await testValidExample([
         'Lorem ipsum dolor sit amet',
         'example@example.com.',
      ]);
      await testValidExample([
         'Lorem ipsum dolor sit amet',
         '<https://example.com>.',
      ]);
      await testValidExample([
         '* [Lorem ipsum](https://example.com) dolor sit amet',
         '  consectetur adipiscing elit.',
      ]);
      await testValidExample([
         '* example@example.com email',
         '  consectetur adipiscing elit.',
      ]);
      await testValidExample([
         '* <https://example.com> dolor sit amet',
         '  consectetur adipiscing elit.',
      ]);
      await testValidExample([
         '* [Lorem ipsum](https://example.com) dolor sit amet',
         '  * [Lorem ipsum](https://example.com) consectetur adipiscing elit.',
      ]);
   });

   it('reports no errors for properly struck-out text', async () => {
      await testValidExample([
         'Lorem ipsum dolor sit amet',
         '~~consectetur adipiscing elit~~.',
      ]);
      await testValidExample([
         '* ~~Lorem ipsum~~ dolor sit amet',
         '  consectetur adipiscing elit.',
      ]);
      await testValidExample([
         '* ~~Lorem ipsum~~ dolor sit amet',
         '  * ~~Lorem ipsum~~ consectetur adipiscing elit.',
      ]);
   });

   it('reports no errors for line starting with an escaped character', async () => {
      await testValidExample([
         '\\* Lorem ipsum dolor sit amet',
         'consectetur adipiscing elit.',
      ]);
      await testValidExample([
         '* \\* Lorem ipsum dolor sit amet',
         '  consectetur adipiscing elit.',
      ]);
      await testValidExample([
         '* \\* Lorem ipsum dolor sit amet',
         '  * \\* Lorem ipsum dolor sit amet',
      ]);
   });

   it('reports no errors for properly aligned footnotes', async () => {
      await testValidExample([
         'Lorem ipsum dolor sit amet[^1], consectetur adipiscing elit.',
         '',
         '[^1]: Curabitur ut posuere est, quis convallis orci.',
         '      Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
   });

   it('reports errors for improperly aligned footnotes', async () => {
      await testInvalidExample([
         'Lorem ipsum dolor sit amet[^1], consectetur adipiscing elit.',
         '',
         '[^1]: Curabitur ut posuere est, quis convallis orci.',
         '     Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
      await testInvalidExample([
         'Lorem ipsum dolor sit amet[^1], consectetur adipiscing elit.',
         '',
         '[^1]: Curabitur ut posuere est, quis convallis orci.',
         '       Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
   });

   it('reports errors for improperly aligned blockquotes', async () => {
      await testInvalidExample([
         ' > indented top-level blockquote',
      ]);
      await testInvalidExample([
         '> top-level blockquote',
         '  > indented top-level blockquote line',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '',
         '  > child blockquote list item (-1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '',
         '    > child blockquote list item (1 offset)',
      ]);
      await testInvalidExample([
         '1. first-level list item',
         '  first-level list item',
         '',
         '    > child blockquote list item (1 offset)',
         '      > child blockquote list item (1 offset)',
      ]);
      await testInvalidExample([
         '>> ul item',
         '   >> ul item',
      ]);
   });

   it('reports no errors for indented lists inside double blockquotes', async () => {
      await testValidExample([
         '>> hello',
         '>>',
         '>>   * test',
         '>>   * test',
         '>>',
         '>> test',
         '>>',
         '>>  1. test',
         '>>  2. test',
         '>>  3. test',
      ]);
   });

   it('reports no errors for left-aligned paragraphs', async () => {
      await testValidExample([
         'Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
         'Pellentesque vestibulum lectus non tellus congue,',
         'eu ultricies metus ultrices.',
         '',
         'Curabitur ut posuere est, quis convallis orci.',
         'Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
   });

   it('reports errors for misaligned paragraphs', async () => {
      await testInvalidExample([
         '    Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
         'Pellentesque vestibulum lectus non tellus congue,',
         '  eu ultricies metus ultrices.',
         '',
         'Curabitur ut posuere est, quis convallis orci.',
         '   Nam lectus magna, pellentesque at rutrum in, commodo a nisi.',
      ]);
   });

   it('reports no errors for properly aligned links in lists', async () => {
      await testValidExample([
         '1. [This is a multiline link',
         '   to test](#test-link)',
         '2. [another link](#another-link)',
      ]);
   });

   it('should validate li as a header', async () => {
      await testValidExample([
         '### 1. ol item',
         '   1. ol item',
         '      * ul item',
         '   2. ol item',
         '## 2. ol item',
      ]);
   });

   it('reports no errors for correct indention of item under long ol list item prefixes', async () => {
      const oneHundredItemList = Array.from({ length: 10 }, (_, index) => {
         return `${index + 1}. item ${index + 1}`;
      });

      await testValidExample([
         ...oneHundredItemList,
         '101. item 101',
         '     * sub-item 101',
      ]);
      await testValidExample([
         ...oneHundredItemList,
         '101. item 101',
         '     1. sub-item 101',
      ]);
      await testValidExample([
         ...oneHundredItemList,
         '101. item 101',
         '     wrapped line under item 101',
      ]);
      await testValidExample([
         ...Array.from({ length: 100 }, (_, index) => {
            return `${index + 1}. item ${index + 1}`;
         }),
         '101. item 101',
         '',
         '     > blockquote under item 101',
      ]);
   });

   it('reports no errors for multi-line html tags', async () => {
      await testValidExample([
         '<a class="this is a long attribute which needs to be wrapped"',
         'href="#">This query</a>',
         'testing',
      ]);

      await testValidExample([
         '<div class="this is a long attribute which needs to be wrapped">',
         '   Lorem ipsum dolor sit amet',
         '</div>',
      ]);

      await testValidExample([
         '<a',
         'href="#">',
         '   link text',
         '</a>',
      ]);
   });

   it('reports errors for incorrect indention of item under long ol list item prefixes', async () => {
      const oneHundredItemList = Array.from({ length: 10 }, (_, index) => {
         return `${index + 1}. item ${index + 1}`;
      });

      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '    * sub-item 101 (-1 offset)',
      ]);
      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '      * sub-item 101 (+1 offset)',
      ]);
      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '    1. sub-item 101 (-1 offset)',
      ]);
      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '      1. sub-item 101 (+1 offset)',
      ]);
      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '    wrapped line under item 1011 (-1 offset)',
      ]);
      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '      wrapped line under item 101 (+1 offset)',
      ]);
      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '    > sub-item 101 (-1 offset)',
      ]);
      await testInvalidExample([
         ...oneHundredItemList,
         '101. item 101',
         '      > sub-item 101 (+1 offset)',
      ]);
   });

});
