package org.tapirscan;
import java.nio.file.Files;
import java.nio.file.Path;
public final class Smoke {
    private static String escape(String text) {
        StringBuilder out=new StringBuilder();
        for(char c:text.toCharArray()) {
            if(c=='"' || c=='\\') out.append('\\').append(c);
            else if(c<32) out.append(String.format(java.util.Locale.ROOT,"\\u%04x",(int)c));
            else out.append(c);
        }
        return out.toString();
    }
    public static void main(String[] args) throws Exception {
        if(args.length!=7 && args.length!=9 && args.length!=10) throw new IllegalArgumentException("libdir mode width height channels stride pixels [multiple include_regions]");
        byte[] pixels=Files.readAllBytes(Path.of(args[6]));
        int width=Integer.parseInt(args[2]),height=Integer.parseInt(args[3]),channels=Integer.parseInt(args[4]);
        long stride=Long.parseLong(args[5]);
        Tapirscan scanner=new Tapirscan(Path.of(args[0]),Tapirscan.Mode.valueOf(args[1].replace('-','_').toUpperCase()));
        Tapirscan.ScanResult result;
        try(scanner; var opposite=new Tapirscan(Path.of(args[0]),
                scanner.mode()==Tapirscan.Mode.MEDIUM ? Tapirscan.Mode.HIGH : Tapirscan.Mode.MEDIUM)) {
            var other=opposite.scan(pixels,width,height,channels,stride);
            String otherMode=opposite.mode().name().toLowerCase(java.util.Locale.ROOT);
            if(!other.json().contains("\"mode\":\""+otherMode+"\"")) throw new AssertionError("Mode libraries collided");
            var options=args.length>=9 ? new Tapirscan.ScanOptions(args[7].equals("1"),args[8].equals("1"),args.length==10 ? Integer.parseInt(args[9]) : 1) : Tapirscan.ScanOptions.defaults();
            result=scanner.scan(pixels,width,height,channels,stride,options);
            for(var b:result.barcodes()) {
                if(b.format().isEmpty() || b.polygon().size()!=4) throw new AssertionError("Invalid typed result");
            }
            try { scanner.scan(new byte[1],width,height,channels,stride); throw new AssertionError("Short input accepted"); }
            catch(IllegalArgumentException expected) {}
        }
        scanner.close();
        try { scanner.scan(pixels,width,height,channels,stride); throw new AssertionError("Closed scanner accepted"); }
        catch(IllegalStateException expected) {}
        // Managed result remains usable after the native scanner has closed.
        if(result.best().isPresent() && result.barcodes().isEmpty()) throw new AssertionError();
        StringBuilder typed=new StringBuilder();
        for(var b:result.barcodes()) {
            if(typed.length()>0) typed.append(',');
            typed.append("{\"text\":\"").append(escape(b.text())).append("\",\"support\":").append(b.support()).append(",\"polygon\":[");
            boolean first=true;
            for(var p:b.polygon()) {
                if(!first) typed.append(',');first=false;
                typed.append('[').append(p.x()).append(',').append(p.y()).append(']');
            }
            typed.append("]}");
        }
        System.out.println("{\"result\":"+result.json()+",\"typed\":["+typed+"]}");
    }
}
