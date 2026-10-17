// esbuild inlines images imported by the chip content script as data: URLs.
declare module "*.webp" {
  const url: string;
  export default url;
}
