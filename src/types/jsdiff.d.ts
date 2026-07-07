declare module 'jsdiff' {
  interface DiffPart {
    added?: boolean;
    removed?: boolean;
    value: string;
  }
  export function diffLines(oldStr: string, newStr: string, options?: { newlineIsToken?: boolean }): DiffPart[];
}
