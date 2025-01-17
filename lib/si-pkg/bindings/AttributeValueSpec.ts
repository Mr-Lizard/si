// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { AttrFuncInputSpec } from "./AttrFuncInputSpec";
import type { AttributeValuePath } from "./AttributeValuePath";
import type { FuncSpecBackendKind } from "./FuncSpecBackendKind";
import type { FuncSpecBackendResponseType } from "./FuncSpecBackendResponseType";
import type { JsonValue } from "./serde_json/JsonValue";

export type AttributeValueSpec = {
  parentPath: AttributeValuePath | null;
  path: AttributeValuePath;
  funcUniqueId: string;
  funcBindingArgs: JsonValue;
  handler: string | null;
  backendKind: FuncSpecBackendKind;
  responseType: FuncSpecBackendResponseType;
  codeBase64: string | null;
  unprocessedValue: JsonValue | null;
  value: JsonValue | null;
  outputStream: JsonValue | null;
  isProxy: boolean;
  sealedProxy: boolean;
  componentSpecific: boolean;
  inputs: Array<AttrFuncInputSpec>;
  implicitValue: JsonValue | null;
};