'use strict';

const { addErrorDetailIf } = require('markdownlint-rule-helpers');

function traverse(tokens, nodeTypes, iterateeFn) {
   tokens.forEach((token) => {
      if (!nodeTypes || nodeTypes.includes(token.type)) {
         iterateeFn(token);
      }

      // Avoid linting HTML blocks because the commonmark spec does not define the proper
      // parent/child structure that would be needed to validate blocks of html text. For
      // example, a div that's inside another div is still considered a sibling by the
      // commonmark standard. See also: https://spec.commonmark.org/0.31.2/#html-blocks
      if (token.children && token.type !== 'htmlFlow') {
         traverse(token.children, nodeTypes, iterateeFn);
      }
   });
}

function iterate(tokens, nodeTypes, iterateeFn) {
   let skipTo;

   (tokens || []).forEach((token) => {
      if (skipTo && skipTo === token.type) {
         skipTo = undefined;
      }

      // If an HTML tag spans multiple lines, and is not considered a "starting condition"
      // it will be parsed as part of a paragraph instead of an HTML block. For example:
      // ```md
      // <a
      // href="">this text is inside a paragraph.</a>
      //
      // <a href="">this text is inside a block of HTML.</a>
      // ```
      // See "starting condition" for more details:
      // https://spec.commonmark.org/0.31.2/#html-blocks
      //
      // Since we want to avoid linting any HTML content, we skip any tokens that appear
      // on the same line as `htmlText`. Note that if we did not do this, any text before
      // or after `htmlText` could be inaccurately flagged by this rule.
      if (token.type === 'htmlText') {
         skipTo = 'lineEnding';
      }

      const isSkippingTokens = skipTo !== undefined,
            visitRequested = !nodeTypes || nodeTypes.includes(token.type);

      if (!isSkippingTokens && visitRequested) {
         iterateeFn(token, (requestedSkipTo) => {
            skipTo = requestedSkipTo;
         });
      }
   });
}

function findFirstNonHTMLTokenOfType(tokens, nodeTypes) {
   let skipLine;

   return (tokens || []).find((token) => {
      if (token.type === 'htmlText') {
         skipLine = token.endLine;
      }
      return nodeTypes.includes(token.type) && token.startLine !== skipLine;
   });
}

function getReadableNameOfTokenForError(token) {
   const names = {
      blockQuote: 'blockquotes',
      codeFenced: 'code blocks',
      content: 'paragraphs',
      listOrdered: 'ordered lists',
      listUnordered: 'unordered lists',
   };

   return names[token.type] || token.type;
}

module.exports = {

   names: [ 'indent-alignment' ],
   description: 'Indent alignment of list items, wrapped lines, and blocks',
   information: new URL('https://github.com/silvermine/markdownlint-rule-indent-alignment'),
   tags: [ 'bullet', 'ul', 'il', 'indentation', 'paragraph' ],
   parser: 'micromark',

   'function': function listIndentation(params, onError) {
      // Ensure top-level blocks and paragraphs are not indented
      iterate(params.parsers.micromark.tokens, [ 'blockQuote', 'codeFenced', 'content' ], (token) => {
         addErrorDetailIf(
            onError,
            token.startLine,
            0,
            token.startColumn - 1,
            `Top-level ${getReadableNameOfTokenForError(token)} should not be indented`,
            undefined,
            [ 1, token.startColumn - 1 ],
            {
               deleteCount: token.startColumn - 1,
            }
         );
      });

      // Ensure top-level lists adhere to the start_indent setting if it`s defined
      if (params.config.start_indent !== undefined) {
         iterate(params.parsers.micromark.tokens, [ 'listOrdered', 'listUnordered' ], (token) => {
            const startColumn = token.startColumn - 1;

            addErrorDetailIf(
               onError,
               token.startLine,
               params.config.start_indent,
               startColumn,
               `Top-level ${getReadableNameOfTokenForError(token)} should be indented ${params.config.start_indent} spaces.`,
               undefined,
               [ 1, Math.max(startColumn, 1) ],
               {
                  deleteCount: startColumn,
                  insertText: ' '.repeat(token.startColumn - 1),
               }
            );
         });
      }

      // Ensure all text lines within a paragraph are aligned
      traverse(params.parsers.micromark.tokens, [ 'paragraph' ], (token) => {
         const inlineTextLikeTokens = [
            'autolink',
            'characterEscape',
            'codeText',
            'data',
            'emphasis',
            'link',
            'literalAutolink',
            'strong',
         ];

         const firstToken = findFirstNonHTMLTokenOfType(token.children, inlineTextLikeTokens);

         if (!firstToken) {
            return;
         }

         const expectedIndent = firstToken.startColumn - 1; // -1 to make zero indexed

         iterate(token.children, inlineTextLikeTokens, (child, skipTo) => {
            const startColumn = child.startColumn - 1; // -1 to make zero indexed

            addErrorDetailIf(
               onError,
               child.startLine,
               expectedIndent,
               startColumn,
               'Wrapped text should be left-aligned with the preceding content',
               undefined,
               [ 1, Math.max(startColumn, 1) ],
               {
                  deleteCount: startColumn,
                  insertText: ' '.repeat(expectedIndent),
               }
            );
            skipTo('lineEnding');
         });
      });

      // Ensure blocks are aligned
      traverse(params.parsers.micromark.tokens, [ 'blockQuote', 'listOrdered', 'listUnordered' ], (token) => {
         let iterateOverChildTokens = [ 'codeFenced', 'content', 'listItemPrefix', 'blockQuote', 'listOrdered', 'listUnordered' ];

         const firstToken = findFirstNonHTMLTokenOfType(token.children, [ 'content' ]);

         if (!firstToken) {
            return;
         }

         // This rule does not enforce a specific initial indentation of list items
         // since that behavior is handled by rules like MD007. Therefore, ignore any
         // list tokens that are the direct children of blockQuote tokens.
         // We also ignore nested blockQuotes because the checks below are not written
         // to handle that edge case.
         if (token.type === 'blockQuote') {
            iterateOverChildTokens = [ 'codeFenced', 'content', 'listItemPrefix' ];
         }

         let expectedIndent = firstToken.startColumn - 1; // -1 to make zero indexed

         iterate(token.children, iterateOverChildTokens, (child, skipTo) => {
            const startColumn = child.startColumn - 1; // -1 to make zero indexed

            if (child.type === 'listItemPrefix') {
               addErrorDetailIf(
                  onError,
                  child.startLine,
                  token.startColumn - 1,
                  startColumn,
                  'List items should be left-aligned with the preceding list items',
                  undefined,
                  [ 1, Math.max(startColumn, 1) ],
                  {
                     deleteCount: startColumn,
                     insertText: ' '.repeat(token.startColumn - 1),
                  }
               );

               expectedIndent = token.startColumn + child.endColumn - child.startColumn - 1;
               skipTo('lineEnding');
               return;
            }

            // When `ul_indent` is configured, allow sub-items to be indented at a
            // different depth. This is to support behavior found in ul-indent (MD007).
            // See: https://github.com/DavidAnson/markdownlint/blob/main/doc/md007.md
            if (params.config.ul_indent && token.type === 'listUnordered' && [ 'listOrdered', 'listUnordered' ].includes(child.type)) {
               const expectedListIndent = (token.startColumn - 1) + params.config.ul_indent;

               addErrorDetailIf(
                  onError,
                  child.startLine,
                  expectedListIndent,
                  startColumn,
                  `Child ${getReadableNameOfTokenForError(child)} should be indented ${params.config.ul_indent} from the parent list`,
                  undefined,
                  [ 1, startColumn ],
                  {
                     deleteCount: startColumn,
                     insertText: ' '.repeat(expectedListIndent),
                  }
               );
               return;
            }

            addErrorDetailIf(
               onError,
               child.startLine,
               expectedIndent,
               startColumn,
               `Nested ${getReadableNameOfTokenForError(child)} should be left-aligned with the preceding content`,
               undefined,
               [ 1, startColumn ],
               {
                  deleteCount: startColumn,
                  insertText: ' '.repeat(expectedIndent),
               }
            );
         });
      });
   },

};
