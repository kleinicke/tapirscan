#include "tapirscan.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>
int main(void) {
    assert(sizeof(barcode_read)==104);
    assert(sizeof(barcode_result_metadata)==24);
    assert(barcode_abi_version()==3);
    uint8_t pixels[4096]; memset(pixels,255,sizeof(pixels));
    tapirscan_handle scanner=0; barcode_result result=123;
    assert(tapirscan_create(&scanner)==0);
    assert(barcode_scan(scanner,pixels,1,64,64,1,64,&result)==BARCODE_INVALID_ARGUMENT);
    assert(result==0);
    assert(barcode_scan_with_options(scanner,pixels,4096,64,64,1,64,BARCODE_INCLUDE_REGIONS,&result)==0);
    assert(tapirscan_destroy(scanner)==0);
    assert(tapirscan_destroy(scanner)==BARCODE_INVALID_HANDLE);
    barcode_result_metadata info;
    assert(barcode_result_info(result,&info)==0);
    assert(info.barcode_count==0);
    uint8_t* json=malloc(info.json_length+1); assert(json);
    assert(barcode_result_copy_json(result,json,info.json_length)==BARCODE_BUFFER_TOO_SMALL);
    assert(barcode_result_copy_json(result,json,info.json_length+1)==0);
    assert(strstr((char*)json,"\"searchWindows\""));
    assert(json[info.json_length]==0);
    assert(barcode_result_destroy(result)==0);
    assert(barcode_result_info(result,&info)==BARCODE_INVALID_HANDLE);
    free(json); return 0;
}
